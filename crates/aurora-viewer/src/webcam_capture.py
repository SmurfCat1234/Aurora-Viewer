"""Local companion for Aurora's webcam tracking."""

import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import json
from queue import Empty, Queue
import sys
from threading import Condition, Lock, Thread
import time


FACE_INDICES = (263, 33, 10, 152, 13, 14, 61, 291, 362, 386, 374, 473, 133, 159, 145, 468)
# Arms, paired hips, knees, ankles and toe landmarks.
BODY_INDICES = (11, 13, 15, 12, 14, 16, 23, 24, 25, 26, 27, 28, 31, 32)
PREVIEW_SIZE = 384
TRACKING_SIZE = 480


def points(landmarks, indices, aspect=1.0):
    # SL avatar coordinates: +X forward, +Y avatar left, +Z up.
    return [[-p.z, p.x, -p.y * aspect] for p in (landmarks.landmark[i] for i in indices)]


def list_cameras():
    import cv2
    from cv2_enumerate_cameras import enumerate_cameras

    # OBS Virtual Camera is available through DirectShow on Windows.
    backend = {
        "win32": cv2.CAP_DSHOW,
        "linux": cv2.CAP_V4L2,
        "darwin": cv2.CAP_AVFOUNDATION,
    }.get(sys.platform, cv2.CAP_ANY)
    print(json.dumps([
        {"index": device.index, "backend": backend, "name": device.name}
        for device in enumerate_cameras(backend)
    ]))


def resize(cv2, image, limit):
    height, width = image.shape[:2]
    longest = max(width, height)
    if longest <= limit:
        return image
    ratio = limit / longest
    return cv2.resize(image, (max(1, round(width * ratio)), max(1, round(height * ratio))))


def dots(landmarks, step=1, visible_only=False):
    if landmarks is None:
        return []
    return [
        [round(p.x, 4), round(p.y, 4)]
        for p in landmarks.landmark[::step]
        if not visible_only or p.visibility >= 0.55
    ]


def landmarks(body, hands, face, aspect):
    frame = {}
    if body.pose_world_landmarks:
        pose = body.pose_world_landmarks
        frame["body"] = [
            xyz if pose.landmark[i].visibility >= 0.55 else None
            for i, xyz in zip(BODY_INDICES, points(pose, BODY_INDICES))
        ]
    hand_points = hands.multi_hand_landmarks or ()
    for hand, identity in zip(hand_points, hands.multi_handedness or ()):
        # MediaPipe assumes a horizontally mirrored selfie image.
        side = "left" if identity.classification[0].label == "Right" else "right"
        frame[side] = points(hand, range(21), aspect)
    face_points = face.multi_face_landmarks[0] if face.multi_face_landmarks else None
    if face_points:
        frame["face"] = points(face_points, FACE_INDICES, aspect)
    groups = [
        dots(body.pose_landmarks, visible_only=True),
        dots(face_points, 5),
        *(dots(hand) for hand in hand_points),
    ]
    return frame, groups


class Tracking:
    """One pending image. Capture can overwrite it while the models are busy."""

    def __init__(self):
        self.changed = Condition()
        self.pending = None
        self.closed = False
        self.overlay = None

    def submit(self, source_id, timestamp, image, still_preview=None):
        with self.changed:
            self.pending = (source_id, timestamp, image, still_preview)
            self.changed.notify()

    def clear(self):
        with self.changed:
            self.pending = None
            self.overlay = None

    def close(self):
        with self.changed:
            self.closed = True
            self.changed.notify()

    def run(self, cv2, emit):
        import mediapipe as mp
        import numpy as np

        with (
            mp.solutions.pose.Pose(model_complexity=1) as pose,
            mp.solutions.hands.Hands(model_complexity=0) as hands,
            mp.solutions.face_mesh.FaceMesh(refine_landmarks=True) as face,
            ThreadPoolExecutor(max_workers=3) as pool,
        ):
            trackers = (pose, hands, face)
            blank = np.zeros((256, 256, 3), dtype=np.uint8)
            for warmup in [pool.submit(tracker.process, blank) for tracker in trackers]:
                warmup.result()
            emit({"ready": True})
            last_source = None
            while True:
                with self.changed:
                    self.changed.wait_for(lambda: self.pending is not None or self.closed)
                    if self.closed:
                        return
                    source_id, timestamp, image, still_preview = self.pending
                    self.pending = None
                if last_source is not None and last_source != source_id:
                    for tracker in trackers:
                        tracker.reset()
                last_source = source_id
                height, width = image.shape[:2]
                rgb = cv2.cvtColor(resize(cv2, image, TRACKING_SIZE), cv2.COLOR_BGR2RGB)
                rgb.flags.writeable = False
                results = [pool.submit(tracker.process, rgb) for tracker in trackers]
                body, hand_result, face_result = (result.result() for result in results)
                frame, groups = landmarks(body, hand_result, face_result, height / width)
                self.overlay = (source_id, timestamp, groups)
                sample = {"id": source_id, "frame": frame}
                if still_preview:
                    sample["preview"] = {"jpeg": still_preview, "groups": groups}
                emit(sample)


def read_commands(commands):
    try:
        for line in sys.stdin.buffer:
            commands.put(json.loads(line))
    finally:
        commands.put(None)


def preview_jpeg(cv2, image):
    ok, jpeg = cv2.imencode(".jpg", resize(cv2, image, PREVIEW_SIZE), [cv2.IMWRITE_JPEG_QUALITY, 68])
    return base64.b64encode(jpeg).decode("ascii") if ok else None


def image_playback(cv2, commands, tracking, emit, source):
    import numpy as np
    from PIL import Image, ImageOps

    source_id = source["id"]
    try:
        with Image.open(source["image"]) as picture:
            if picture.width * picture.height > 32_000_000:
                raise RuntimeError("Image is larger than 32 megapixels.")
            animated = picture.format == "GIF" and picture.is_animated
            frame_index = 0
            deadline = time.monotonic()
            while True:
                try:
                    return commands.get(timeout=max(0.0, deadline - time.monotonic()))
                except Empty:
                    pass
                try:
                    picture.seek(frame_index)
                except EOFError:
                    if not source["loop_video"]:
                        emit({"id": source_id, "ended": True})
                        return {"stop": True}
                    frame_index = 0
                    picture.seek(0)
                frame = picture if animated else ImageOps.exif_transpose(picture)
                rgba = frame.convert("RGBA")
                rgba.thumbnail((TRACKING_SIZE, TRACKING_SIZE))
                background = Image.new("RGBA", rgba.size, (128, 128, 128, 255))
                rgb = Image.alpha_composite(background, rgba).convert("RGB")
                image = cv2.cvtColor(np.asarray(rgb), cv2.COLOR_RGB2BGR)
                timestamp = time.monotonic()
                jpeg = preview_jpeg(cv2, image)
                sample = {"id": source_id}
                if not animated:
                    sample["still"] = True
                if jpeg:
                    overlay = tracking.overlay
                    groups = overlay[2] if animated and overlay and overlay[0] == source_id and timestamp - overlay[1] < 0.15 else []
                    sample["preview"] = {"jpeg": jpeg, "groups": groups}
                emit(sample)
                tracking.submit(source_id, timestamp, image, jpeg if not animated else None)
                if not animated:
                    break
                duration = max(10, picture.info.get("duration") or 100) / 1000
                deadline += duration
                # Keep the GIF's per-frame delays without replaying a backlog
                # of overdue frames after a stall.
                if deadline < timestamp:
                    deadline = timestamp + duration
                frame_index += 1
        return commands.get()
    except (OSError, Image.DecompressionBombError) as error:
        raise RuntimeError(f"Cannot read the selected image: {error}") from error
    finally:
        tracking.clear()


def playback(cv2, commands, tracking, emit, source):
    if "image" in source:
        return image_playback(cv2, commands, tracking, emit, source)
    source_id = source["id"]
    video = source.get("video")
    if video is not None and hasattr(cv2, "CAP_PROP_N_THREADS"):
        # Frame-threaded decoding introduces buffering and can delay a switch.
        camera = cv2.VideoCapture(video, cv2.CAP_FFMPEG, [cv2.CAP_PROP_N_THREADS, 1])
        if not camera.isOpened():
            camera.open(video)
    elif video is not None:
        camera = cv2.VideoCapture(video)
    else:
        camera = cv2.VideoCapture(source["camera"], source["backend"])
    try:
        if not camera.isOpened():
            raise RuntimeError("Cannot open the selected input.")
        if video is None:
            camera.set(cv2.CAP_PROP_FRAME_WIDTH, 640)
            camera.set(cv2.CAP_PROP_FRAME_HEIGHT, 480)
        fps = camera.get(cv2.CAP_PROP_FPS)
        interval = 1.0 / fps if 1 <= fps <= 240 else 1.0 / 30
        next_frame = time.monotonic()
        next_preview = 0.0
        video_started = False
        while True:
            now = time.monotonic()
            try:
                return commands.get(timeout=max(0.0, next_frame - now) if video is not None else 0)
            except Empty:
                pass
            if video is not None:
                now = time.monotonic()
                # Keep the file on its clock. Drop outdated pictures if decoding stalls.
                overdue = min(5, int(max(0.0, now - next_frame) / interval))
                for _ in range(overdue):
                    if not camera.grab():
                        break
                next_frame = max(next_frame + (1 + overdue) * interval, now)
            ok, image = camera.read()
            if not ok and video is not None and source["loop_video"]:
                camera.set(cv2.CAP_PROP_POS_FRAMES, 0)
                ok, image = camera.read()
                video_started = False
                if not ok:
                    raise RuntimeError("Cannot loop the selected video.")
            if not ok:
                if video is None or next_preview == 0.0:
                    raise RuntimeError("The selected input did not return a frame.")
                emit({"id": source_id, "ended": True})
                return {"stop": True}
            if video is not None and not video_started:
                emit({"id": source_id, "video_started": True})
                video_started = True
            timestamp = time.monotonic()
            tracking.submit(source_id, timestamp, image)
            if timestamp >= next_preview:
                jpeg = preview_jpeg(cv2, image)
                if jpeg:
                    overlay = tracking.overlay
                    groups = overlay[2] if overlay and overlay[0] == source_id and timestamp - overlay[1] < 0.15 else []
                    emit({
                        "id": source_id,
                        "preview": {"jpeg": jpeg, "groups": groups},
                    })
                # Keep a phase on typical 24/30 FPS sources without starving high-rate cameras.
                preview_interval = 1.0 / 30
                next_preview = max(next_preview + preview_interval, timestamp - preview_interval * 0.1) if next_preview else timestamp
    finally:
        camera.release()
        tracking.clear()


def worker():
    import cv2
    cv2.setNumThreads(1)

    output_lock = Lock()

    def emit(sample):
        line = json.dumps(sample, allow_nan=False, separators=(",", ":"))
        with output_lock:
            print(line, flush=True)

    commands = Queue()
    tracking = Tracking()

    def infer():
        try:
            tracking.run(cv2, emit)
        except Exception as error:
            emit({"fatal": str(error)})
            commands.put(None)

    Thread(target=read_commands, args=(commands,), daemon=True).start()
    inference = Thread(target=infer, name="landmarks")
    inference.start()
    try:
        source = commands.get()
        while source is not None:
            if source.get("stop"):
                source = commands.get()
                continue
            try:
                source = playback(cv2, commands, tracking, emit, source)
            except (ImportError, RuntimeError, cv2.error) as error:
                emit({"id": source["id"], "error": str(error)})
                source = commands.get()
    finally:
        tracking.close()
        inference.join()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--list-cameras", action="store_true")
    options = parser.parse_args()
    try:
        if options.list_cameras:
            list_cameras()
        else:
            worker()
    except (ImportError, RuntimeError) as error:
        sys.exit(str(error))
