//! The real request page layout. No API, polling or configuration reads.
use super::{
    controls::{Button, ButtonKind, Choice, TextCtrl, paint_surface},
    theme,
};
use crate::desktop::clients::ClientId;
use wxdragon::prelude::*;
pub(super) struct RequestView {
    pub page: Panel,
    pub search: TextCtrl,
    pub client: Choice,
    pub apply: Button,
    pub reset: Button,
    pub technical: CheckBox,
    pub result: Choice,
    pub channel: TextCtrl,
    pub start: TextCtrl,
    pub end: TextCtrl,
    pub table: ListCtrl,
    pub status: StaticText,
    pub previous: Button,
    pub next: Button,
}
impl RequestView {
    pub fn new<W: WxWidget>(
        parent: &W,
        values: [&str; 4],
        client_index: u32,
        result_index: u32,
    ) -> Self {
        let page = Panel::builder(parent).build();
        page.set_background_color(theme::theme().bg_app);
        let column = BoxSizer::builder(Orientation::Vertical).build();
        let toolbar = WrapSizer::builder(Orientation::Horizontal)
            .with_flags(WrapSizerFlag::RemoveLeadingSpaces)
            .build();
        let search = TextCtrl::builder(&page)
            .with_value(values[0])
            .with_size(Size::new(180, theme::CONTROL_HEIGHT))
            .build();
        search.set_tooltip("搜索模型、渠道或请求编号");
        search.set_hint("搜索模型、渠道、请求");
        let mut choices = vec!["全部客户端".into()];
        choices.extend(ClientId::ALL.into_iter().map(|c| c.name().into()));
        let client = Choice::builder(&page)
            .with_choices(choices)
            .with_size(Size::new(160, theme::CONTROL_HEIGHT))
            .build();
        client.set_selection(client_index);
        let apply = Button::builder(&page)
            .with_label("筛选")
            .with_kind(ButtonKind::Secondary)
            .with_size(Size::new(60, theme::CONTROL_HEIGHT))
            .build();
        let reset = Button::builder(&page)
            .with_label("清除")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(52, theme::CONTROL_HEIGHT))
            .build();
        let advanced = Button::builder(&page)
            .with_label("更多条件")
            .with_kind(ButtonKind::Ghost)
            .with_size(Size::new(88, theme::CONTROL_HEIGHT))
            .build();
        let technical = CheckBox::builder(&page).with_label("技术列").build();
        technical.set_foreground_color(theme::theme().ink_primary);
        let item_style = SizerFlag::Right | SizerFlag::Bottom | SizerFlag::AlignCenterVertical;
        toolbar.add(&search, 0, item_style, page.from_dip(8));
        toolbar.add(&client, 0, item_style, page.from_dip(8));
        for b in [apply, reset, advanced] {
            toolbar.add(&b, 0, item_style, page.from_dip(8));
        }
        toolbar.add(&technical, 0, SizerFlag::AlignCenterVertical, 0);
        column.add_sizer(
            &toolbar,
            0,
            SizerFlag::Expand | SizerFlag::Bottom,
            page.from_dip(8),
        );
        let more = Panel::builder(&page).build();
        more.set_background_color(theme::theme().bg_app);
        let grid = FlexGridSizer::builder(0, 4)
            .with_hgap(10)
            .with_vgap(10)
            .build();
        grid.add_growable_col(1, 1);
        grid.add_growable_col(3, 1);
        let result = Choice::builder(&more)
            .with_choices(vec![
                "全部结果".into(),
                "成功".into(),
                "失败".into(),
                "进行中".into(),
                "已取消".into(),
                "未完整返回".into(),
            ])
            .build();
        result.set_selection(result_index);
        let channel = TextCtrl::builder(&more).with_value(values[1]).build();
        let start = TextCtrl::builder(&more).with_value(values[2]).build();
        let end = TextCtrl::builder(&more).with_value(values[3]).build();
        start.set_tooltip("YYYY-MM-DD，包含当天");
        end.set_tooltip("YYYY-MM-DD，包含当天");
        filter_field(&more, &grid, "结果", &result);
        filter_field(&more, &grid, "渠道", &channel);
        filter_field(&more, &grid, "开始日期", &start);
        filter_field(&more, &grid, "结束日期", &end);
        more.set_sizer(grid, true);
        more.hide();
        column.add(
            &more,
            0,
            SizerFlag::Expand | SizerFlag::Left | SizerFlag::Right | SizerFlag::Bottom,
            12,
        );
        advanced.on_click(move |_| {
            more.show(!more.is_shown());
            page.layout();
        });
        let surface = Panel::builder(&page).build();
        paint_surface(surface, theme::SURFACE_RADIUS);
        let surface_col = BoxSizer::builder(Orientation::Vertical).build();
        let table = ListCtrl::builder(&surface)
            .with_style(
                ListCtrlStyle::Report | ListCtrlStyle::SingleSel | ListCtrlStyle::BorderNone,
            )
            .build();
        table.set_background_color(theme::theme().bg_card);
        table.set_foreground_color(theme::theme().ink_primary);
        for (i, (title, width)) in [
            ("时间", 150),
            ("客户端", 125),
            ("模型", 170),
            ("渠道", 155),
            ("状态", 90),
            ("输入", 90),
            ("输出", 90),
            ("耗时", 85),
        ]
        .into_iter()
        .enumerate()
        {
            table.insert_column(i as i64, title, ListColumnFormat::Left, width);
        }
        table.on_size(move |event| {
            fit_columns(table, false);
            event.skip(true);
        });
        surface_col.add(
            &table,
            1,
            SizerFlag::Expand | SizerFlag::All,
            surface.from_dip(8),
        );
        surface.set_sizer(surface_col, false);
        column.add(&surface, 1, SizerFlag::Expand, 0);
        let footer = BoxSizer::builder(Orientation::Horizontal).build();
        let status = StaticText::builder(&page).with_label("").build();
        status.set_foreground_color(theme::theme().ink_secondary);
        status.wrap(520);
        let previous = Button::builder(&page).with_label("上一页").build();
        let next = Button::builder(&page).with_label("下一页").build();
        footer.add(
            &status,
            1,
            SizerFlag::AlignCenterVertical | SizerFlag::Right,
            12,
        );
        footer.add(&previous, 0, SizerFlag::Right, 8);
        footer.add(&next, 0, SizerFlag::default(), 0);
        column.add_sizer(
            &footer,
            0,
            SizerFlag::Expand | SizerFlag::Top,
            page.from_dip(12),
        );
        page.set_sizer(column, true);
        Self {
            page,
            search,
            client,
            apply,
            reset,
            technical,
            result,
            channel,
            start,
            end,
            table,
            status,
            previous,
            next,
        }
    }
}
pub(super) fn fit_columns(table: ListCtrl, technical: bool) {
    // Leave the extra technical columns scrollable; normal columns fill the
    // actual client area, including a maximized window and DPI-scaled widths.
    let width = (table.get_client_size().width - table.from_dip(4)).max(table.from_dip(792));
    for (i, weight) in [18, 17, 18, 15, 8, 8, 8, 8].into_iter().enumerate() {
        table.set_column_width(i as i64, width * weight / 100);
    }
    if technical {
        for i in 8..12 {
            table.set_column_width(i, table.from_dip(if i < 10 { 100 } else { 230 }));
        }
    }
}
fn filter_field<W: WxWidget>(parent: &Panel, grid: &FlexGridSizer, name: &str, input: &W) {
    let label = StaticText::builder(parent).with_label(name).build();
    label.set_foreground_color(theme::theme().ink_secondary);
    grid.add(&label, 0, SizerFlag::AlignCenterVertical, 0);
    grid.add(input, 1, SizerFlag::Expand, 0);
}
