//! The provider editor's real field layout, independently renderable without
//! configuration, credentials, network, or a modal event loop.
use super::{controls::TextCtrl, theme, visual};
use wxdragon::prelude::*;

pub(super) struct ProviderFields {
    pub grid: FlexGridSizer,
    pub inputs: [TextCtrl; 6],
    pub chat_disable_reasoning: CheckBox,
}
impl ProviderFields {
    pub fn new<W: WxWidget>(
        parent: &W,
        labels: [&str; 6],
        protocol: &str,
        reasoning: &str,
        models_help: &str,
    ) -> Self {
        let grid = FlexGridSizer::builder(0, 2)
            .with_vgap(parent.from_dip(10))
            .with_hgap(parent.from_dip(14))
            .build();
        grid.add_growable_col(1, 1);
        let inputs = std::array::from_fn(|i| {
            let label = StaticText::builder(parent).with_label(labels[i]).build();
            label.set_foreground_color(theme::theme().ink_secondary);
            label.wrap(parent.from_dip(118));
            grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
            let input = TextCtrl::builder(parent)
                .with_value(if i == 0 {
                    protocol
                } else if i == 5 {
                    "100"
                } else {
                    ""
                })
                .build();
            input.set_min_size(visual::dip_size(&input, 220, 36));
            grid.add(&input, 1, SizerFlag::Expand, 0);
            input
        });
        inputs[0].set_editable(false);
        inputs[3].set_tooltip(models_help);
        let spacer = StaticText::builder(parent).with_label("").build();
        grid.add(&spacer, 0, SizerFlag::default(), 0);
        let chat_disable_reasoning = CheckBox::builder(parent).with_label(reasoning).build();
        chat_disable_reasoning.set_foreground_color(theme::theme().ink_primary);
        grid.add(&chat_disable_reasoning, 0, SizerFlag::Expand, 0);
        Self {
            grid,
            inputs,
            chat_disable_reasoning,
        }
    }
}
