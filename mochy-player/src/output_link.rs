//! OutputRouter integration for the mixer (the blueprint's seam: the
//! backends call [`Mixer::render`] through [`RenderSource`]).

use mochy_audio::output::RenderSource;

use crate::mixer::Mixer;

impl RenderSource for Mixer {
    fn render(&mut self, out: &mut [f32], channels: usize) {
        debug_assert_eq!(
            out.len() % channels.max(1),
            0,
            "render blocks are whole frames"
        );
        if channels == self.channels() {
            Mixer::render(self, out);
        } else {
            // Channel-count drift between the negotiated stream and the
            // mixer: render at the mixer's layout into scratch and
            // route (mono duplication / drop extras). Not a path the
            // router produces, but it must never panic mid-callback.
            let mixer_channels = self.channels();
            let frames = out.len() / channels.max(1);
            let mut scratch = vec![0f32; frames * mixer_channels];
            Mixer::render(self, &mut scratch);
            for frame in 0..frames {
                for channel in 0..channels {
                    let value = if channel < mixer_channels {
                        scratch[frame * mixer_channels + channel]
                    } else {
                        scratch[frame * mixer_channels]
                    };
                    out[frame * channels + channel] = value;
                }
            }
        }
    }
}
