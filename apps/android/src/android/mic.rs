//! The phone's microphone (Android's AAudio), read a tenth of a second at a time and sent to the computer's phone
//! microphone (`POST /stage-view/mic/sound?rate=…`, 16-bit mono), exactly as `web/phone-mic.html` does. The sound
//! goes as it is: the stream's own filters clean it up.
use ndk::audio::{AudioDirection, AudioFormat, AudioPerformanceMode, AudioSharingMode, AudioStream, AudioStreamBuilder};

pub struct Mic { stream: AudioStream, pub rate: u32 }

impl Mic {
    pub fn open() -> Result<Mic, String> {
        let stream = AudioStreamBuilder::new().map_err(|e| e.to_string())?
            .direction(AudioDirection::Input)
            .channel_count(1)
            .format(AudioFormat::PCM_I16)
            .sample_rate(48_000)
            .sharing_mode(AudioSharingMode::Shared)
            .performance_mode(AudioPerformanceMode::None)
            .open_stream().map_err(|e| e.to_string())?;
        stream.request_start().map_err(|e| e.to_string())?;
        let rate = u32::try_from(stream.sample_rate()).unwrap_or(48_000);
        Ok(Mic { stream, rate })
    }

    /// A tenth of a second of sound (waits for it).
    pub fn read(&self) -> Result<Vec<i16>, String> {
        let frames = (self.rate / 10) as usize;
        let mut buf = vec![0i16; frames];
        let mut got = 0usize;
        while got < frames {
            let n = unsafe { self.stream.read(buf[got..].as_mut_ptr().cast(), (frames - got) as i32, 500_000_000) }.map_err(|e| e.to_string())?;
            if n == 0 { break }
            got += n as usize;
        }
        buf.truncate(got);
        Ok(buf)
    }
}

impl Drop for Mic {
    fn drop(&mut self) { let _ = self.stream.request_stop(); }
}
