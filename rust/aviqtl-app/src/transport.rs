use std::time::Instant;

#[derive(Debug, Clone, Copy)]
struct PlaybackAnchor {
    time: Instant,
    frame: i32,
}

/// Frame-accurate transport clock independent of egui's repaint cadence.
#[derive(Debug)]
pub struct Transport {
    playing: bool,
    playback_speed: f64,
    anchor: Option<PlaybackAnchor>,
    was_playing_before_scrub: bool,
    last_scrub_frame: Option<i32>,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            playback_speed: 1.0,
            anchor: None,
            was_playing_before_scrub: false,
            last_scrub_frame: None,
        }
    }
}

impl Transport {
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn playback_speed(&self) -> f64 {
        self.playback_speed
    }

    pub fn set_playback_speed(&mut self, speed: f64) {
        if speed.is_finite() {
            self.playback_speed = speed.clamp(0.1, 4.0);
        }
    }

    pub fn toggle(&mut self, now: Instant, playhead: &mut i32, end_frame: i32) {
        if self.playing {
            self.pause();
        } else {
            self.play(now, playhead, end_frame);
        }
    }

    pub fn play(&mut self, now: Instant, playhead: &mut i32, end_frame: i32) {
        self.playing = true;
        self.anchor = Some(PlaybackAnchor {
            time: now,
            frame: (*playhead).clamp(0, end_frame.max(0)),
        });
    }

    pub fn pause(&mut self) {
        self.playing = false;
        self.anchor = None;
    }

    pub fn seek(&mut self, now: Instant, playhead: i32) {
        if self.playing {
            self.anchor = Some(PlaybackAnchor {
                time: now,
                frame: playhead,
            });
        }
    }

    pub fn begin_scrub(&mut self) {
        self.was_playing_before_scrub = self.playing;
        if self.playing {
            self.pause();
        }
        self.last_scrub_frame = None;
    }

    pub fn scrub_to(&mut self, now: Instant, frame: i32, playhead: &mut i32) -> bool {
        let frame = frame.max(0);
        if self.last_scrub_frame == Some(frame) {
            return false;
        }
        self.last_scrub_frame = Some(frame);
        *playhead = frame;
        self.seek(now, frame);
        true
    }

    pub fn end_scrub(&mut self, now: Instant, playhead: i32) {
        self.last_scrub_frame = None;
        if self.was_playing_before_scrub {
            self.playing = true;
            self.anchor = Some(PlaybackAnchor {
                time: now,
                frame: playhead,
            });
        }
        self.was_playing_before_scrub = false;
    }

    pub fn step(&mut self, delta: i32, playhead: &mut i32, end_frame: i32) {
        if self.playing {
            return;
        }
        *playhead = playhead.saturating_add(delta).clamp(0, end_frame.max(0));
    }

    pub fn update(&mut self, now: Instant, fps: f64, end_frame: i32, playhead: &mut i32) -> bool {
        if !self.playing {
            return false;
        }
        if !fps.is_finite() || fps <= 0.0 || end_frame <= 0 {
            self.pause();
            return false;
        }
        let anchor = self.anchor.get_or_insert(PlaybackAnchor {
            time: now,
            frame: *playhead,
        });
        let elapsed_frames =
            (now.saturating_duration_since(anchor.time).as_secs_f64() * fps * self.playback_speed)
                .floor()
                .min(f64::from(i32::MAX)) as i32;
        let target = anchor.frame.saturating_add(elapsed_frames);
        let next = target.min(end_frame);
        let changed = next != *playhead;
        *playhead = next;
        if target >= end_frame {
            self.pause();
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn playback_uses_elapsed_time_and_stops_on_the_last_frame() {
        let start = Instant::now();
        let mut transport = Transport::default();
        let mut playhead = 10;
        transport.play(start, &mut playhead, 100);
        assert!(transport.update(start + Duration::from_millis(500), 30.0, 100, &mut playhead));
        assert_eq!(playhead, 25);
        assert!(transport.is_playing());

        assert!(transport.update(start + Duration::from_secs(4), 30.0, 100, &mut playhead));
        assert_eq!(playhead, 100);
        assert!(!transport.is_playing());
    }

    #[test]
    fn seeking_reanchors_playback_and_steps_are_clamped() {
        let start = Instant::now();
        let mut transport = Transport::default();
        let mut playhead = 99;
        transport.play(start, &mut playhead, 100);
        assert_eq!(playhead, 99);

        playhead = 40;
        transport.seek(start + Duration::from_secs(1), playhead);
        transport.update(
            start + Duration::from_millis(1500),
            20.0,
            100,
            &mut playhead,
        );
        assert_eq!(playhead, 50);

        transport.pause();
        transport.step(-100, &mut playhead, 100);
        assert_eq!(playhead, 0);
        assert!(!transport.is_playing());
        transport.step(200, &mut playhead, 100);
        assert_eq!(playhead, 100);
    }

    #[test]
    fn scrubbing_pauses_seeks_and_resumes_prior_playback() {
        let start = Instant::now();
        let mut transport = Transport::default();
        let mut playhead = 10;
        transport.play(start, &mut playhead, 100);

        transport.begin_scrub();
        assert!(!transport.is_playing());
        assert!(transport.scrub_to(start + Duration::from_millis(100), 40, &mut playhead));
        assert_eq!(playhead, 40);
        assert!(!transport.scrub_to(start + Duration::from_millis(200), 40, &mut playhead));

        transport.end_scrub(start + Duration::from_millis(300), playhead);
        assert!(transport.is_playing());
        assert!(transport.update(start + Duration::from_millis(800), 20.0, 100, &mut playhead));
        assert_eq!(playhead, 50);
    }

    #[test]
    fn playback_speed_and_playing_step_behavior_match_qt() {
        let start = Instant::now();
        let mut transport = Transport::default();
        let mut playhead = 10;
        transport.set_playback_speed(2.0);
        transport.play(start, &mut playhead, 100);
        assert!(transport.update(start + Duration::from_millis(500), 30.0, 100, &mut playhead));
        assert_eq!(playhead, 40);

        transport.step(1, &mut playhead, 100);
        assert_eq!(playhead, 40);
        assert!(transport.is_playing());

        transport.pause();
        transport.step(100, &mut playhead, 100);
        assert_eq!(playhead, 100);
        transport.play(start, &mut playhead, 100);
        assert!(!transport.update(start, 30.0, 100, &mut playhead));
        assert_eq!(playhead, 100);
        assert!(!transport.is_playing());
    }
}
