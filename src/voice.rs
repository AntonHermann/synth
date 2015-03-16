//!
//!  synth_voice.rs
//!
//!  Created by Mitchell Nordine at 04:01PM on June 28, 2014.
//!
//!

use dsp::Settings as DspSettings;
use dsp::{Sample};
use oscillator::Oscillator;
use time::{self, Samples};
use env_point::Point;
use envelope::Envelope;
use waveform::Waveform;

pub type Playhead = time::calc::Samples;
pub type LoopStart = time::calc::Samples;
pub type LoopEnd = time::calc::Samples;
pub type Attack = time::calc::Samples;
pub type Release = time::calc::Samples;
pub type LoopPlayhead = time::calc::Samples;
pub type NoteDuration = time::calc::Samples;
pub type NoteFreqMulti = f64;
pub type NoteHz = f32;
pub type NoteVelocity = f32;

/// A single Voice. A Synth may consist
/// of any number of Voices.
#[derive(Clone, Debug, RustcEncodable, RustcDecodable)]
pub struct Voice {
    /// Oscillators for playback.
    pub oscillators: Vec<Oscillator>,
    /// Data for a note, if there is one currently being played.
    pub maybe_note: Option<(NoteState, NoteHz, NoteFreqMulti, NoteVelocity)>,
    /// Playhead over the current note.
    pub playhead: Playhead,
    /// Playhead over the loop duration.
    pub loop_playhead: LoopPlayhead,
}

/// The current state of the Voice's note playback.
#[derive(Copy, Clone, Debug, RustcEncodable, RustcDecodable)]
pub enum NoteState {
    /// The note is current playing.
    Playing,
    /// The note has been released and is fading out.
    Released(Playhead),
}

impl Voice {

    /// Constructor for a Voice.
    pub fn new(oscillators: Vec<Oscillator>) -> Voice {
        Voice {
            oscillators: oscillators,
            maybe_note: None,
            playhead: 0,
            loop_playhead: 0,
        }
    }

    /// Default constructor for a Voice with a single Oscillator.
    pub fn default() -> Voice {
        Voice::new(vec!(Oscillator::new()))
    }

    /// Testing constructor. Creates a basic Kick sound.
    pub fn test_demo() -> Voice {
        let amp_env = Envelope::from_points(vec!(
            Point::new(0.0,  0.0, 0.0),
            Point::new(0.01, 1.0, 0.0),
            Point::new(0.45, 1.0, 0.0),
            Point::new(0.81, 0.8, 0.0),
            Point::new(1.0,  0.0, 0.0),
        ));
        let freq_env = Envelope::from_points(vec!(
            Point::new(0.0,     0.0,    0.0),
            Point::new(0.00136, 1.0   , 0.0),
            Point::new(0.015  , 0.01  , 0.0),
            Point::new(0.045  , 0.005 , 0.0),
            Point::new(0.1    , 0.0022, 0.0),
            Point::new(0.35   , 0.0011, 0.0),
            Point::new(1.0,     0.0,    0.0),
        ));
        let oscillator = Oscillator::new()
            .waveform(Waveform::Sine)
            .amplitude(amp_env)
            .frequency(freq_env);

        Voice::new(vec!(oscillator))
    }

    /// Trigger playback with the given note, resetting all playheads.
    #[inline]
    pub fn note_on(&mut self, hz: NoteHz, freq_multi: NoteFreqMulti, vel: NoteVelocity) {
        self.maybe_note = Some((NoteState::Playing, hz, freq_multi, vel));
        self.playhead = 0;
        self.loop_playhead = 0;
    }

    /// Release playback of the current not eif there is one.
    #[inline]
    pub fn note_off(&mut self) {
        self.maybe_note = self.maybe_note.map(|(_, h, m, v)| (NoteState::Released(0), h, m, v));
    }

    /// Stop playback of the current note if there is one and reset the playheads.
    #[inline]
    pub fn stop(&mut self) {
        self.maybe_note = None;
        self.playhead = 0;
        self.loop_playhead = 0;
    }

    /// Generate and fill the audio buffer for the given parameters.
    #[inline]
    pub fn fill_buffer<S>(&mut self,
                          output: &mut [S],
                          settings: DspSettings,
                          duration: time::calc::Samples,
                          loop_data: Option<&(LoopStart, LoopEnd)>,
                          fade_data: Option<&(Attack, Release)>)
        where S: Sample
    {
        let Voice {
            ref mut oscillators,
            ref mut playhead,
            ref mut loop_playhead,
            ref mut maybe_note,
        } = *self;

        let (attack, release) = fade_data.map_or_else(|| (0, 0), |&(a, r)| (a, r));
        let (freq_multi, velocity) = maybe_note.map_or_else(|| (1.0, 1.0), |(_, _, m, v)| (m, v));

        for frame in output.chunks_mut(settings.channels as usize) {

            // Calculate the amplitude of the current frame.
            let wave = if maybe_note.is_some() && *loop_playhead < duration {
                let ratio = *loop_playhead as f64 / duration as f64;
                let note_state = maybe_note.map(|(state, _, _, _)| state).unwrap();
                // Sum the amplitude of each oscillator at the given ratio.
                oscillators.iter_mut().fold(0.0, |total, osc| {
                    let mut wave = osc.amp_at_ratio(ratio,
                                                    freq_multi,
                                                    settings.sample_hz as f64);
                    // If within the attack duration, apply the fade.
                    if *playhead < attack {
                        wave *= *playhead as f32 / attack as f32;
                    }
                    // If within the release duration, apply the fade.
                    if let NoteState::Released(release_playhead) = note_state {
                        wave *= (release - release_playhead) as f32 / release as f32;
                    }
                    wave + total
                })
            } else {
                // If the playhead is out of range or if there is no note, zero the frame.
                0.0
            };

            // Assign the amp to each channel.
            for channel in frame.iter_mut() {
                *channel = Sample::from_wave(wave * velocity);
            }

            // Iterate the release playhead and check for whether or not the release playhead
            // exceeds the release limit. If it does, reset the note.
            let note_should_reset = match *maybe_note {
                Some((NoteState::Released(ref mut release_playhead), _, _, _)) => {
                    *release_playhead += 1;
                    *release_playhead > release
                },
                None => continue,
                _ => false,
            };
            if note_should_reset {
                *maybe_note = None;
                *playhead = 0;
            }

            // Iterate the loop_playhead. If the loop_playhead passes the loop_end,
            // reset the playhead to the start.
            *loop_playhead += 1;
            if let Some(&(loop_start, loop_end)) = loop_data {
                if *loop_playhead >= loop_end {
                    *loop_playhead = (*loop_playhead - loop_end) + loop_start;
                }
            }

            // Iterate the playhead. If the playhead passes the duration of the instrument or
            // the note that is currently being played, reset the playhead and stop playback.
            *playhead += 1;
            if *playhead >= duration + release || *loop_playhead > duration {
                *maybe_note = None;
                *playhead = 0;
            }

        }

    }

}

