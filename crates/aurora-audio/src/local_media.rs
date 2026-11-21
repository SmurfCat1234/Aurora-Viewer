//! Bounded, cancellable playback of a local video soundtrack.

use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;

use crate::{
    AudioError, Channel, ENGINE_SAMPLE_RATE,
    decode::DecodeStream,
    mixer::{Command, PcmSource},
    resample::{Resampler, to_stereo},
};

/// Owns the playback of one video pass. Drop it when playback stops or loops.
pub struct LocalMediaAudio(Arc<AtomicBool>);

impl LocalMediaAudio {
    pub(crate) fn start(
        path: PathBuf,
        started: Instant,
        offset: f64,
        rate: f64,
        mute: Arc<AtomicBool>,
        tx: Sender<Command>,
    ) -> Option<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        thread::Builder::new()
            .name("local-media-audio".into())
            .spawn(move || {
                if let Err(error) = play(&path, started, offset, rate, stop, mute, tx) {
                    // Silent and unsupported clips still play their video.
                    log::debug!("local video audio unavailable: {error}");
                }
            })
            .ok()?;
        Some(Self(cancelled))
    }
}

impl Drop for LocalMediaAudio {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn play(
    path: &Path,
    started: Instant,
    offset: f64,
    rate: f64,
    stop: Arc<AtomicBool>,
    mute: Arc<AtomicBool>,
    tx: Sender<Command>,
) -> Result<(), AudioError> {
    let file = File::open(path).map_err(|error| AudioError::Decode(error.to_string()))?;
    let mut decoder = DecodeStream::open(Box::new(file), path.extension().and_then(|ext| ext.to_str()))?;
    // A failed demuxer seek can invalidate the stream. Decode from the start
    // on a fresh reader in that case.
    let mut skip_source = if offset >= 0.1 {
        if let Some(remaining) = decoder.seek_before(offset) {
            remaining
        } else {
            let file = File::open(path).map_err(|error| AudioError::Decode(error.to_string()))?;
            decoder = DecodeStream::open(Box::new(file), path.extension().and_then(|ext| ext.to_str()))?;
            offset
        }
    } else {
        offset
    };
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }

    let (mut producer, consumer) = rtrb::RingBuffer::new(4_800 * 2);
    let source = PcmSource::new(Channel::Media, consumer, 480, None, Some(stop.clone())).with_mute(mute);
    if tx.try_send(Command::AddPcm(Box::new(source))).is_err() {
        return Ok(());
    }

    // Opening a file should not delay the pictures. If probing was slow,
    // start with the samples that correspond to the current video position.
    let skip = started.elapsed().saturating_sub(Duration::from_millis(30));
    skip_source += skip.as_secs_f64() * rate;
    let mut skip_frames = (skip_source / rate * f64::from(ENGINE_SAMPLE_RATE)) as usize;
    let mut stereo = Vec::new();
    let mut converted = Vec::new();
    let mut resampler = None;
    while !stop.load(Ordering::Relaxed) && !producer.is_abandoned() {
        let Some(chunk) = decoder.next_chunk()? else { break };
        stereo.clear();
        to_stereo(chunk.samples, chunk.channels, &mut stereo);
        let input_rate = (f64::from(chunk.rate) * rate).round() as u32;
        let pcm = if input_rate == ENGINE_SAMPLE_RATE {
            resampler = None;
            &stereo
        } else {
            let (rate, converter) = resampler.get_or_insert_with(|| (input_rate, Resampler::new(input_rate, ENGINE_SAMPLE_RATE, 2)));
            if *rate != input_rate {
                *rate = input_rate;
                *converter = Resampler::new(input_rate, ENGINE_SAMPLE_RATE, 2);
            }
            converted.clear();
            converter.process(&stereo, &mut converted);
            &converted
        };
        let skip = skip_frames.min(pcm.len() / 2);
        skip_frames -= skip;
        let mut rest = &pcm[skip * 2..];
        while !rest.is_empty() {
            if stop.load(Ordering::Relaxed) || producer.is_abandoned() {
                return Ok(());
            }
            let (_, remaining) = producer.push_partial_slice(rest);
            rest = remaining;
            if !rest.is_empty() {
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
    Ok(())
}
