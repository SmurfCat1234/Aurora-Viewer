"""Local companion for Aurora's webcam tracking."""

import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import json
import math
from queue import Empty, Queue
import sys
from threading import Condition, Lock, Thread
import time


FACE_INDICES = (263, 33, 10, 152, 13, 14, 61, 291, 362, 386, 374, 473, 133, 159, 145, 468)
# Arms, paired hips, knees, ankles and toe landmarks.
BODY_INDICES = (11, 13, 15, 12, 14, 16, 23, 24, 25, 26, 27, 28, 31, 32)
PREVIEW_SIZE = 384
TRACKING_SIZE = 480
FILE_TRACKING_SIZE = 720


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


def visible(point):
    return point.visibility >= 0.55 and 0 <= point.x <= 1 and 0 <= point.y <= 1


def distance(a, b, aspect):
    return math.hypot(a.x - b.x, (a.y - b.y) * aspect)


def head_crop(cv2, image, pose):
    if not pose:
        return image, None
    p = pose.landmark
    if not visible(p[0]):
        return image, None
    h, w = image.shape[:2]
    aspect = h / w
    eye_span = distance(p[2], p[5], aspect)
    shoulder_span = distance(p[11], p[12], aspect)
    side = max(min(w, h) * 0.12, w * eye_span * 5.5, w * shoulder_span * 0.55)
    x = round(p[0].x * w)
    y = round(p[0].y * h)
    half = round(side / 2)
    x0, x1 = max(0, x - half), min(w, x + half)
    y0, y1 = max(0, y - half), min(h, y + half)
    crop = image[y0:y1, x0:x1]
    if crop.size == 0:
        return image, None
    if max(crop.shape[:2]) < 192:
        factor = 192 / max(crop.shape[:2])
        crop = cv2.resize(crop, None, fx=factor, fy=factor)
    elif not crop.flags.c_contiguous:
        crop = crop.copy()
    return crop, (x0 / w, y0 / h, (x1 - x0) / w, (y1 - y0) / h)


def anchored_face(face, region, pose, aspect):
    if not face.multi_face_landmarks:
        return None
    face_points = face.multi_face_landmarks[0]
    f = face_points.landmark
    if region:
        x, y, w, h = region
        for point in f:
            point.x = x + point.x * w
            point.y = y + point.y * h
            point.z *= w
    if not pose:
        return face_points
    p = pose.landmark
    if not visible(p[0]):
        return None if visible(p[11]) and visible(p[12]) else face_points
    eyes = max(distance(p[2], p[5], aspect), 0.012)
    face_width = distance(f[33], f[263], aspect)
    if distance(p[0], f[1], aspect) > max(0.02, eyes * 1.25) or not 0.35 * eyes <= face_width <= 2.8 * eyes:
        return None
    return face_points


def anchored_hands(hands, pose, aspect):
    hand_points = hands.multi_hand_landmarks or ()
    p = pose.landmark if pose else None
    if p is None or not (visible(p[15]) or visible(p[16]) or (visible(p[11]) and visible(p[12]))):
        # Labels assume a horizontally mirrored selfie.
        return {
            "left" if identity.classification[0].label == "Right" else "right": hand
            for hand, identity in zip(hand_points, hands.multi_handedness or ())
        }
    candidates = []
    for i, hand in enumerate(hand_points):
        for side, wrist, elbow in (("left", 15, 13), ("right", 16, 14)):
            w = p[wrist]
            if visible(w) or (w.visibility >= 0.35 and visible(p[elbow]) and 0 <= w.x <= 1 and 0 <= w.y <= 1):
                separation = distance(p[wrist], hand.landmark[0], aspect)
                if separation <= max(0.04, min(0.12, distance(p[wrist], p[elbow], aspect) * 0.75)):
                    candidates.append((separation, i, side))
    assigned = {}
    used = set()
    for _, i, side in sorted(candidates):
        if i not in used and side not in assigned:
            assigned[side] = hand_points[i]
            used.add(i)
    return assigned


def pose_is_hand(pose, hands):
    """An isolated hand can resemble a tiny full skeleton to Pose."""
    candidates = hands.multi_hand_landmarks or ()
    if not pose or len(candidates) != 1:
        return False
    hand = candidates[0].landmark
    x0, x1 = min(p.x for p in hand), max(p.x for p in hand)
    y0, y1 = min(p.y for p in hand), max(p.y for p in hand)
    if max(x1 - x0, y1 - y0) < 0.30:
        return False
    torso = (pose.landmark[i] for i in (0, 11, 12, 23, 24))
    return sum(x0 <= p.x <= x1 and y0 <= p.y <= y1 for p in torso) >= 4


def landmarks(body, hands, face_points, aspect):
    frame = {}
    pose2d = body.pose_landmarks
    if pose_is_hand(pose2d, hands):
        pose2d = None
        face_points = None
    if body.pose_world_landmarks and pose2d:
        pose = body.pose_world_landmarks
        frame["body"] = [
            xyz if visible(pose2d.landmark[i]) and all(math.isfinite(value) for value in xyz) else None
            for i, xyz in zip(BODY_INDICES, points(pose, BODY_INDICES))
        ]
    hand_points = anchored_hands(hands, pose2d, aspect)
    for side, hand in hand_points.items():
        frame[side] = points(hand, range(21), aspect)
    if face_points:
        frame["face"] = points(face_points, FACE_INDICES, aspect)
    groups = [
        dots(pose2d, visible_only=True),
        dots(face_points, 5),
        *(dots(hand_points.get(side)) for side in ("left", "right")),
    ]
    return frame, groups


class StableLandmarks:
    """Ride through brief occlusions and require a second observation for large jumps."""

    def __init__(self):
        self.body = {}
        self.pending = {}
        self.detail = {}

    def apply(self, frame, timestamp):
        body = frame.get("body", [None] * len(BODY_INDICES))
        for i, position in enumerate(body):
            previous = self.body.get(i)
            age = timestamp - previous[0] if previous else math.inf
            if position is None:
                self.pending.pop(i, None)
            elif age < 0.18 and math.dist(previous[1], position) > 0.30 + 2.0 * age:
                candidate = self.pending.get(i)
                self.pending[i] = (timestamp, position)
                if not candidate or timestamp - candidate[0] > 0.15 or math.dist(candidate[1], position) > 0.18:
                    position = None
            if position is not None:
                self.body[i] = (timestamp, position)
                self.pending.pop(i, None)
            elif age <= 0.12:
                body[i] = previous[1]
            else:
                body[i] = None
        if any(position is not None for position in body):
            frame["body"] = body
        for name in ("face", "left", "right"):
            if name in frame:
                self.detail[name] = (timestamp, frame[name])
            elif name in self.detail and timestamp - self.detail[name][0] <= 0.12:
                frame[name] = self.detail[name][1]


class Tracking:
    """One pending image. Capture can overwrite it while the models are busy."""

    def __init__(self):
        self.changed = Condition()
        self.pending = None
        self.closed = False
        self.overlay = None
        self.revision = 0

    def submit(self, source_id, timestamp, image, still_preview=None, is_file=False):
        with self.changed:
            self.pending = (source_id, self.revision, timestamp, image, still_preview, is_file)
            self.changed.notify()

    def clear(self):
        with self.changed:
            self.pending = None
            self.overlay = None
            self.revision += 1

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
            previous_pose = None
            cropped = False
            stable = StableLandmarks()
            while True:
                with self.changed:
                    self.changed.wait_for(lambda: self.pending is not None or self.closed)
                    if self.closed:
                        return
                    source_id, revision, timestamp, image, still_preview, is_file = self.pending
                    self.pending = None
                source = (source_id, revision)
                if last_source is not None and last_source != source:
                    for tracker in trackers:
                        tracker.reset()
                    previous_pose = None
                    cropped = False
                    stable = StableLandmarks()
                last_source = source
                height, width = image.shape[:2]
                rgb = cv2.cvtColor(resize(cv2, image, FILE_TRACKING_SIZE if is_file else TRACKING_SIZE), cv2.COLOR_BGR2RGB)
                rgb.flags.writeable = False
                body_task = pool.submit(pose.process, rgb)
                hand_task = pool.submit(hands.process, rgb)
                anchor = previous_pose if previous_pose is not None else body_task.result().pose_landmarks
                face_image, region = head_crop(cv2, rgb, anchor)
                if bool(region) != cropped:
                    face.reset()
                    cropped = bool(region)
                face_task = pool.submit(face.process, face_image)
                body = body_task.result()
                previous_pose = body.pose_landmarks
                face_points = anchored_face(face_task.result(), region, previous_pose, height / width)
                frame, groups = landmarks(body, hand_task.result(), face_points, height / width)
                if still_preview is None:
                    stable.apply(frame, timestamp)
                sample = {"id": source_id, "frame": frame}
                if still_preview:
                    sample["preview"] = {"jpeg": still_preview, "groups": groups}
                with self.changed:
                    if revision == self.revision:
                        self.overlay = (source_id, timestamp, groups)
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
                    tracking.clear()
                frame = picture if animated else ImageOps.exif_transpose(picture)
                rgba = frame.convert("RGBA")
                rgba.thumbnail((FILE_TRACKING_SIZE, FILE_TRACKING_SIZE))
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
                tracking.submit(source_id, timestamp, image, jpeg if not animated else None, is_file=True)
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
                tracking.clear()
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
            tracking.submit(source_id, timestamp, image, is_file=video is not None)
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
