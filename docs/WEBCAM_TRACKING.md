# Webcam tracking prototype

Build › Webcam Tracking animates your own avatar in the viewer. The camera
starts on request and stops when you close the window, log out or quit. The
companion runs inference on this computer and sends landmarks to Aurora over
its private stdout pipe. Camera frames stay in memory. The viewer sends
its normal avatar animations to the grid.

## Try it

1. With Python 3.12 installed, use PowerShell on Windows to create a venv,
   install the tested packages, and launch the demo.

   ```powershell
   py -3.12 -m venv "$env:LOCALAPPDATA\AuroraWebcam\venv"
   $env:AURORA_WEBCAM_PYTHON = "$env:LOCALAPPDATA\AuroraWebcam\venv\Scripts\python.exe"
   & $env:AURORA_WEBCAM_PYTHON -m pip install 'mediapipe==0.10.21' 'opencv-contrib-python==4.11.0.86'
   $env:AURORA_DEMO = '1'
   cargo run -p aurora-viewer -- --title 'Test local-webcam-tracking'
   ```

   Launch the viewer from the same shell so it inherits the interpreter
   path. With the dependencies installed in the default `python` interpreter
   on your PATH, the environment variable is optional.
2. Open **Build › Webcam Tracking**, choose the camera index (usually 0),
   and select **Start camera**. Face the webcam with your arms in view. Look
   forward with your eyes open and mouth relaxed, then select **Set neutral
   pose**. **Mirror motion** swaps sides.
3. Turn and tilt your head, blink, open your mouth, separately lift each arm,
   turn the wrist, and make a fist. Move out of frame, then select **Stop
   tracking** and verify that the usual animation takes over. Use a Bento
   rigged head and hands weighted to the corresponding joints for a
   meaningful in-world comparison.

Set `AURORA_DEMO_WEBCAM_VIDEO` to a local video file before launching the
offline demo to test without a camera. The window shows **Replay test video**
in place of **Start camera**.

The prototype estimates head motion, gaze, blink, jaw opening, chest and
arm rotations, palm orientation and finger curl. Camera selection is by
numeric index. Camera preview, per-channel controls, dedicated OBS output,
expression controls beyond the supported joints, and motion recording are
outside this prototype. A normal OBS window capture can show the viewer's
rendered output.

## Implementation and inspiration

The MediaPipe Pose, Hands and Face Mesh detectors run in an optional Python
companion. A bounded reader keeps only the latest frame. Aurora applies
tracked rotations to its own avatar's render skeleton after blending normal
animations and before composing the joint palette. The Poser and animation
editing tools retain priority. The underlying animation keeps running.

Thanks to [@miyumiyuna5](https://x.com/miyumiyuna5/status/2116499758420512852)
for demonstrating face and upper-body tracking in Aurora. This implementation
was developed independently from that public demonstration.

Linden Lab's [Puppetry documentation](https://wiki.secondlife.com/wiki/How_Puppetry_Works)
and [LEAP webcam examples](https://github.com/secondlife/leap/tree/c6aac3e2df8ca30e845864a62b8cecdc94bf8e05/puppetry/webcam)
describe the separate experimental viewer/region stack. The Aurora preview
uses its own local render layer and requires no region feature.
