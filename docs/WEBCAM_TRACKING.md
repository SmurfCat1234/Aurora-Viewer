# Webcam tracking prototype

Build › Webcam Tracking animates your own avatar in the viewer. The camera
starts on request and stops when you close the window, log out or quit. The
companion runs inference on this computer and sends landmarks and small
preview frames to Aurora over its private stdout pipe. Camera frames stay in memory. The viewer sends
its normal avatar animations to the grid.

## Try it

1. With Python 3.12 installed, use PowerShell on Windows to create a venv,
   install the tested packages, and launch the demo.

   ```powershell
   py -3.12 -m venv "$env:LOCALAPPDATA\AuroraWebcam\venv"
   $env:AURORA_WEBCAM_PYTHON = "$env:LOCALAPPDATA\AuroraWebcam\venv\Scripts\python.exe"
   & $env:AURORA_WEBCAM_PYTHON -m pip install 'mediapipe==0.10.21' 'opencv-contrib-python==4.11.0.86' 'cv2-enumerate-cameras==1.3.0'
   $env:AURORA_DEMO = '1'
   cargo run -p aurora-viewer -- --title 'Test local-webcam-tracking'
   ```

   Launch the viewer from the same shell so it inherits the interpreter
   path. With the dependencies installed in the default `python` interpreter
   on your PATH, the environment variable is optional.
2. Open **Build › Webcam Tracking**, choose a camera from **Input source**,
   and select **Start camera**. **Refresh cameras** lists attached devices,
   including OBS Virtual Camera on Windows. **Next camera** cycles through
   the devices. Switching inputs during capture restarts tracking on the new
   source and recalibrates the neutral pose. With no named devices, try
   the numbered OpenCV inputs. The camera enumeration package is optional
   for numbered camera input and local replay.
3. Face the webcam with your arms in view. Look forward with your eyes open
   and mouth relaxed, then select **Set neutral pose**. **Mirror motion**
   swaps sides in the preview and avatar. **Front view** selects Aurora's
   front camera preset. Use the channel checkboxes to
   isolate head, eyes, expressions, hands or upper body. Expression strength
   changes the jaw and eyelid response.
4. Turn and tilt your head, blink, open your mouth, separately lift each arm,
   turn the wrist, and make a fist. Move out of frame, then select **Stop
   tracking** and verify that the usual animation takes over. Use a Bento
   rigged head and hands weighted to the corresponding joints for a
   meaningful in-world comparison.

Choose **Open video…** to feed a local clip through the same trackers as the
camera. **Loop video** repeats the clip. Set `AURORA_DEMO_WEBCAM_VIDEO` before
launching the offline demo to preselect a clip. **Preview** and **Landmarks**
show the input image and detected points. The preview keeps its place while
idle, opening a source, or switching inputs. Once the selected source
returns a frame, Aurora shows it while the tracking models finish starting.
The status reports the approximate
rate at which the viewer receives tracking samples.

The prototype estimates head motion, gaze, blink, jaw opening, chest and
arm rotations, palm orientation and finger curl. Full-body clips can exercise
the pose detector and show leg landmarks in the preview. The avatar's legs
continue to play their normal animation. A normal OBS window capture can
show the viewer's rendered output.

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

LEAP is an open client-side plug-in interface. A viewer launches a child
process and exchanges length-prefixed LLSD messages over its standard
streams. The official Puppetry example sends joint targets over that
interface. The experimental viewer's Puppetry module turns them into
animation updates. A capable region must relay those updates for other
clients to draw them. Linden Lab publishes the viewer and the LEAP
library. The simulator source is outside that release.

[OpenSimulator](https://github.com/opensim/opensim) is an independent BSD
licensed server that can run on Linux. An isolated grid experiment would
need an opt-in server capability and corresponding viewer support for
transmitting, receiving and applying joint updates. Installing the LEAP
library by itself changes only the local plug-in side.
