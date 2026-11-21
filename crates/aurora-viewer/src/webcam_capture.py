"""Local companion for Aurora's webcam tracking."""

import argparse
import base64
import json
from pathlib import Path
import sys
import time


def points(landmarks, aspect=1.0):
    # SL avatar coordinates: +X forward, +Y avatar left, +Z up.
    return [[-p.z, p.x, -p.y * aspect] for p in landmarks.landmark]


def list_cameras():
    import cv2
    from cv2_enumerate_cameras import enumerate_cameras

    # OBS Virtual Camera is available through DirectShow on Windows.
    backend = {
        "win32": cv2.CAP_DSHOW,
        "linux": cv2.CAP_V4L2,
        "darwin": cv2.CAP_AVFOUNDATION,
    }.get(sys.platform, cv2.CAP_ANY)
    info = enumerate_cameras(backend)
    print(json.dumps([
        {"index": device.index, "backend": backend, "name": device.name}
        for device in info
    ]))


def preview_frame(cv2, image, body, hands, face):
    height, width = image.shape[:2]
    ratio = min(1.0, 384 / max(width, height))
    small = cv2.resize(image, (max(1, round(width * ratio)), max(1, round(height * ratio))))
    ok, jpeg = cv2.imencode(".jpg", small, [cv2.IMWRITE_JPEG_QUALITY, 68])
    if not ok:
        return None

    def dots(landmarks, step=1, visible_only=False):
        if landmarks is None:
            return []
        return [
            [round(p.x, 4), round(p.y, 4)]
            for p in landmarks.landmark[::step]
            if not visible_only or p.visibility >= 0.55
        ]

    return {
        "jpeg": base64.b64encode(jpeg).decode("ascii"),
        "groups": [
            dots(body.pose_landmarks, visible_only=True),
            dots(face.multi_face_landmarks[0], 5) if face.multi_face_landmarks else [],
            *[dots(hand) for hand in (hands.multi_hand_landmarks or ())],
        ],
    }


def capture(args):
    import cv2
    import mediapipe as mp

    if args.video and not Path(args.video).is_file():
        raise RuntimeError("The demo clip must be a local file.")
    source = args.video if args.video else args.camera
    camera = cv2.VideoCapture(source, args.backend if not args.video else cv2.CAP_ANY)
    try:
        if not camera.isOpened():
            raise RuntimeError("Cannot open the selected camera.")
        if not args.video:
            camera.set(cv2.CAP_PROP_FRAME_WIDTH, 640)
            camera.set(cv2.CAP_PROP_FRAME_HEIGHT, 480)
        fps = camera.get(cv2.CAP_PROP_FPS)
        interval = 1.0 / fps if 1 <= fps <= 240 else 1.0 / 30
        next_preview = 0.0
        # Separate detectors let a close-up keep face and hand tracking when
        # the pose model cannot see enough of the torso or legs.
        with (
            mp.solutions.pose.Pose(model_complexity=1) as body_tracker,
            mp.solutions.hands.Hands(model_complexity=0) as hand_tracker,
            mp.solutions.face_mesh.FaceMesh(refine_landmarks=True) as face_tracker,
        ):
            while True:
                started = time.monotonic()
                ok, bgr = camera.read()
                if not ok and args.video and args.loop:
                    camera.set(cv2.CAP_PROP_POS_FRAMES, 0)
                    ok, bgr = camera.read()
                if not ok:
                    if args.video:
                        return
                    raise RuntimeError("The selected camera stopped returning frames.")
                height, width = bgr.shape[:2]
                aspect = height / width
                rgb = cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)
                rgb.flags.writeable = False
                body = body_tracker.process(rgb)
                hands = hand_tracker.process(rgb)
                face_result = face_tracker.process(rgb)
                frame = {}
                if body.pose_world_landmarks:
                    pose = body.pose_world_landmarks
                    xyz = points(pose)
                    # Left shoulder, elbow, wrist, then right; hips last.
                    frame["body"] = [
                        xyz[i] if pose.landmark[i].visibility >= 0.55 else None
                        for i in (11, 13, 15, 12, 14, 16, 23, 24)
                    ]
                for hand, identity in zip(
                    hands.multi_hand_landmarks or (), hands.multi_handedness or ()
                ):
                    # MediaPipe assumes a horizontally mirrored selfie image.
                    side = "left" if identity.classification[0].label == "Right" else "right"
                    frame[side] = points(hand, aspect)
                if face_result.multi_face_landmarks:
                    face = points(face_result.multi_face_landmarks[0], aspect)
                    # Eye corners, forehead/chin, inner lips, lids and irises.
                    frame["face"] = [
                        face[i] for i in (
                            263, 33, 10, 152, 13, 14, 61, 291,
                            362, 386, 374, 473, 133, 159, 145, 468,
                        )
                    ]
                sample = {"frame": frame}
                if args.preview and started >= next_preview:
                    sample["preview"] = preview_frame(cv2, bgr, body, hands, face_result)
                    next_preview = started + 1 / 12
                print(json.dumps(sample, allow_nan=False, separators=(",", ":")), flush=True)
                if args.video:
                    time.sleep(max(0.0, interval - (time.monotonic() - started)))
    finally:
        camera.release()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--camera", type=int, default=0)
    parser.add_argument("--backend", type=int, default=0)
    parser.add_argument("--video", help="Read a local test clip instead of the camera")
    parser.add_argument("--loop", action="store_true")
    parser.add_argument("--preview", action="store_true")
    parser.add_argument("--list-cameras", action="store_true")
    try:
        options = parser.parse_args()
        if options.list_cameras:
            list_cameras()
        else:
            capture(options)
    except (ImportError, RuntimeError) as error:
        sys.exit(str(error))
