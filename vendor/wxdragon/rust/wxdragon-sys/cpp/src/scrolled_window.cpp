#include <wx/wxprec.h>
#include <wx/wx.h>
#include "wx/scrolwin.h"
#include "wx/window.h"
#include "wx/gdicmn.h"
#include "wxdragon.h"
#include <wx/dcbuffer.h>
#include <memory>
extern wxDC* wxd_CreateBufferedPaintDC(wxWindow* window);

// Keep wxScrollHelper's wheel, keyboard, child-focus and virtual-size logic,
// while presenting narrow draggable thumbs instead of large non-client bars.
class WxdScrolledWindow : public wxScrolledWindow {
    wxPanel *vertical = nullptr, *horizontal = nullptr;
    bool moving = false;
    bool indicators = true;
    bool layingOut = false;
    int drag = -1;
    wxSize lastView, lastVirtual;
    wxPoint lastOrigin;
    wxColour lastBackground;
    wxRect Thumb(bool horizontalAxis) const {
        const auto view = GetClientSize();
        const auto full = GetVirtualSize();
        const int length = horizontalAxis ? view.x : view.y;
        const int total = horizontalAxis ? full.x : full.y;
        const int thickness = FromDIP(8), inset = FromDIP(2);
        int ux, uy; GetScrollPixelsPerUnit(&ux, &uy);
        if ((horizontalAxis ? ux : uy) <= 0) return wxRect();
        if (length <= 0 || total <= length) return wxRect();
        const int track = std::max(1, length - inset * 2 - thickness);
        const int thumb = std::min(track, std::max(FromDIP(28), track * length / total));
        int sx, sy; GetViewStart(&sx, &sy);
        const int offset = horizontalAxis ? sx * ux : sy * uy;
        const int start = inset + (track - thumb) * offset / std::max(1, total - length);
        return horizontalAxis ? wxRect(start, inset, thumb, FromDIP(4)) : wxRect(inset, start, FromDIP(4), thumb);
    }
    void DragTo(bool axis, int coordinate) {
        const auto view = GetClientSize(), full = GetVirtualSize();
        const auto r = Thumb(axis);
        const int length = axis ? view.x : view.y, total = axis ? full.x : full.y;
        const int thumb = axis ? r.width : r.height;
        const int track = std::max(1, length - FromDIP(12) - thumb);
        int sx, sy, ux, uy; GetViewStart(&sx, &sy); GetScrollPixelsPerUnit(&ux, &uy);
        const int pixels = std::max(0, std::min(track, coordinate - FromDIP(2) - drag)) * std::max(0, total - length) / track;
        if (axis && ux) Scroll(pixels / ux, sy);
        if (!axis && uy) Scroll(sx, pixels / uy);
        UpdateThumbs();
    }
    wxPanel* MakeThumb(bool axis) {
        auto* bar = new wxPanel(this, wxID_ANY, wxDefaultPosition, wxDefaultSize, wxBORDER_NONE);
        bar->SetCanFocus(false);
        bar->SetBackgroundStyle(wxBG_STYLE_PAINT);
        bar->Bind(wxEVT_PAINT, [this, bar, axis](wxPaintEvent&) {
            std::unique_ptr<wxDC> paint(wxd_CreateBufferedPaintDC(bar));
            auto& dc = *paint;
            dc.SetBackground(wxBrush(GetBackgroundColour())); dc.Clear();
            auto r = Thumb(axis); if (r.IsEmpty()) return;
            auto bg = GetBackgroundColour();
            const bool dark = bg.Red() + bg.Green() + bg.Blue() < 384;
            const auto color = dark ? wxColour(99, 119, 131) : wxColour(191, 204, 212);
            dc.SetPen(*wxTRANSPARENT_PEN); dc.SetBrush(wxBrush(color));
            dc.DrawRoundedRectangle(r, FromDIP(2));
        });
        bar->Bind(wxEVT_LEFT_DOWN, [this, bar, axis](wxMouseEvent& e) {
            auto r = Thumb(axis); const auto p = e.GetPosition();
            drag = r.Contains(p) ? (axis ? p.x - r.x : p.y - r.y) : (axis ? r.width : r.height) / 2;
            bar->CaptureMouse(); DragTo(axis, axis ? p.x : p.y);
        });
        bar->Bind(wxEVT_MOTION, [this, bar, axis](wxMouseEvent& e) {
            if (bar->HasCapture() && e.Dragging()) DragTo(axis, axis ? e.GetX() : e.GetY()); else e.Skip();
        });
        bar->Bind(wxEVT_LEFT_UP, [this, bar](wxMouseEvent&) { if (bar->HasCapture()) bar->ReleaseMouse(); drag = -1; });
        bar->Bind(wxEVT_MOUSE_CAPTURE_LOST, [this](wxMouseCaptureLostEvent&) { drag = -1; });
        bar->Bind(wxEVT_MOUSEWHEEL, [this](wxMouseEvent& e) { GetEventHandler()->ProcessEvent(e); });
        return bar;
    }
public:
    void ShowScrollIndicators(bool show) {
        indicators = show;
        lastView = wxSize(-1, -1);
        UpdateThumbs();
    }
    WxdScrolledWindow(wxWindow* parent, wxWindowID id, wxPoint pos, wxSize size, long style)
        : wxScrolledWindow(parent, id, pos, size, (style & ~wxBORDER_MASK) | wxBORDER_NONE) {
        ShowScrollbars(wxSHOW_SB_NEVER, wxSHOW_SB_NEVER);
        vertical = MakeThumb(false); horizontal = MakeThumb(true);
        Bind(wxEVT_SIZE, [this](wxSizeEvent& e) { UpdateThumbs(); e.Skip(); });
        Bind(wxEVT_IDLE, [this](wxIdleEvent& e) { UpdateThumbs(); e.Skip(); });
    }
    void UpdateThumbs() {
        if (moving || !vertical || !horizontal) return;
        const auto view = GetClientSize(), full = GetVirtualSize();
        int sx, sy; GetViewStart(&sx, &sy);
        const wxPoint origin(sx, sy);
        if (view == lastView && full == lastVirtual && origin == lastOrigin && GetBackgroundColour() == lastBackground) return;
        lastView = view; lastVirtual = full; lastOrigin = origin; lastBackground = GetBackgroundColour();
        moving = true;
        const auto s = GetClientSize(); const int thickness = FromDIP(8);
        vertical->SetSize(std::max(0, s.x - thickness), 0, thickness, s.y);
        horizontal->SetSize(0, std::max(0, s.y - thickness), s.x, thickness);
        const bool v = indicators && !Thumb(false).IsEmpty(), h = indicators && !Thumb(true).IsEmpty();
        vertical->Show(v); horizontal->Show(h);
        if (v) { vertical->Raise(); vertical->Refresh(false); }
        if (h) { horizontal->Raise(); horizontal->Refresh(false); }
        moving = false;
    }
    void ScrollWindow(int dx, int dy, const wxRect* r = nullptr) override {
        wxScrolledWindow::ScrollWindow(dx, dy, r); UpdateThumbs();
    }
    bool Layout() override {
        // wxScrollHelper::ScrollLayout uses native-bar visibility to decide
        // whether to use virtual dimensions. Our bars are overlays, so use the
        // enabled scroll rate instead. Otherwise short pages get squeezed.
        if (layingOut) return true;
        if (auto* sizer = GetSizer()) {
            layingOut = true;
            const auto view = GetClientSize();
            int ux, uy; GetScrollPixelsPerUnit(&ux, &uy);
            // Recompute from content, not the previous virtual viewport. The
            // latter only ever grew and left scrollbars after content shrank
            // or a narrow wrapping form was widened again.
            auto minimum = sizer->GetMinSize();
            auto size = wxSize(ux > 0 ? std::max(minimum.x, view.x) : view.x,
                               uy > 0 ? std::max(minimum.y, view.y) : view.y);
            sizer->SetDimension(CalcScrolledPosition(wxPoint(0, 0)), size);
            minimum = sizer->GetMinSize();
            size = wxSize(ux > 0 ? std::max(minimum.x, view.x) : view.x,
                          uy > 0 ? std::max(minimum.y, view.y) : view.y);
            SetVirtualSize(size);
            sizer->SetDimension(CalcScrolledPosition(wxPoint(0, 0)), size);
            layingOut = false;
            UpdateThumbs(); return true;
        }
        return wxScrolledWindow::Layout();
    }
};

extern "C" {

WXD_EXPORTED void
wxd_ScrolledWindow_ShowScrollIndicators(wxd_ScrolledWindow_t* self, bool show) {
    if (auto* modern = dynamic_cast<WxdScrolledWindow*>(reinterpret_cast<wxScrolledWindow*>(self)))
        modern->ShowScrollIndicators(show);
}

WXD_EXPORTED wxd_ScrolledWindow_t*
wxd_ScrolledWindow_Create(wxd_Window_t* parent, wxd_Id id, wxd_Point pos, wxd_Size size,
                          wxd_Style_t style)
{
    wxWindow* parentWin = (wxWindow*)parent;
    wxPoint wxPos(pos.x, pos.y);
    wxSize wxSize(size.width, size.height);
    // wxScrolledWindow name parameter is not usually needed
    wxScrolledWindow* scrolledWin = new WxdScrolledWindow(parentWin, id, wxPos, wxSize, style);
    return (wxd_ScrolledWindow_t*)scrolledWin;
}

WXD_EXPORTED void
wxd_ScrolledWindow_SetScrollRate(wxd_ScrolledWindow_t* self, int xstep, int ystep)
{
    wxScrolledWindow* scrolledWin = (wxScrolledWindow*)self;
    if (scrolledWin) {
        scrolledWin->SetScrollRate(xstep, ystep);
        if (auto* modern = dynamic_cast<WxdScrolledWindow*>(scrolledWin)) modern->UpdateThumbs();
    }
}

WXD_EXPORTED bool
wxd_ScrolledWindow_ForwardMouseWheel(wxd_ScrolledWindow_t* self, wxd_Event_t* event)
{
    if (!self || !event) return false;
    auto* scrolled = reinterpret_cast<wxScrolledWindow*>(self);
    auto* wheel = dynamic_cast<wxMouseEvent*>(reinterpret_cast<wxEvent*>(event));
    if (!wheel || wheel->GetEventType() != wxEVT_MOUSEWHEEL || wheel->ControlDown() ||
        wheel->GetWheelAxis() != wxMOUSE_WHEEL_VERTICAL) return false;
    auto* origin = wxDynamicCast(wheel->GetEventObject(), wxWindow);
    if (origin == scrolled) return false;

    // Preserve native line/page settings and fractional rotation accumulation.
    // Process synchronously; neither the source event nor a child is retained.
    wxMouseEvent forwarded(*wheel);
    if (origin) forwarded.SetPosition(scrolled->ScreenToClient(origin->ClientToScreen(wheel->GetPosition())));
    forwarded.SetEventObject(scrolled);
    forwarded.SetId(scrolled->GetId());
    forwarded.Skip(false);
    return scrolled->GetEventHandler()->ProcessEvent(forwarded);
}

WXD_EXPORTED void
wxd_ScrolledWindow_FitInside(wxd_ScrolledWindow_t* self)
{
    wxScrolledWindow* scrolledWin = (wxScrolledWindow*)self;
    if (scrolledWin) {
        scrolledWin->FitInside();
        if (auto* modern = dynamic_cast<WxdScrolledWindow*>(scrolledWin)) modern->UpdateThumbs();
    }
}

WXD_EXPORTED void
wxd_ScrolledWindow_SetScrollbars(wxd_ScrolledWindow_t* self, int pixelsPerUnitX, int pixelsPerUnitY,
                                 int noUnitsX, int noUnitsY, int xPos, int yPos, bool noRefresh)
{
    wxScrolledWindow* scrolledWin = (wxScrolledWindow*)self;
    if (scrolledWin) {
        scrolledWin->SetScrollbars(pixelsPerUnitX, pixelsPerUnitY, noUnitsX, noUnitsY, xPos, yPos,
                                   noRefresh);
    }
}

WXD_EXPORTED void
wxd_ScrolledWindow_EnableScrolling(wxd_ScrolledWindow_t* self, bool xScrolling, bool yScrolling)
{
    wxScrolledWindow* scrolledWin = (wxScrolledWindow*)self;
    if (scrolledWin) {
        scrolledWin->EnableScrolling(xScrolling, yScrolling);
    }
}

WXD_EXPORTED void
wxd_ScrolledWindow_Scroll_Coord(wxd_ScrolledWindow_t* self, int x, int y)
{
    wxScrolledWindow* scrolledWin = (wxScrolledWindow*)self;
    if (scrolledWin) {
        scrolledWin->Scroll(x, y);
    }
}

WXD_EXPORTED void
wxd_ScrolledWindow_Scroll_Point(wxd_ScrolledWindow_t* self, wxd_Point pt)
{
    wxScrolledWindow* scrolledWin = (wxScrolledWindow*)self;
    if (scrolledWin) {
        scrolledWin->Scroll(wxPoint(pt.x, pt.y));
    }
}

WXD_EXPORTED void
wxd_ScrolledWindow_GetVirtualSize(wxd_ScrolledWindow_t* self, int* w, int* h)
{
    wxScrolledWindow* scrolledWin = (wxScrolledWindow*)self;
    if (scrolledWin && w && h) {
        int width, height;
        scrolledWin->GetVirtualSize(&width, &height);
        *w = width;
        *h = height;
    }
    else if (w) {
        *w = 0;
    }
    else if (h) {
        *h = 0;
    }
}

WXD_EXPORTED void
wxd_ScrolledWindow_GetScrollPixelsPerUnit(wxd_ScrolledWindow_t* self, int* xUnit, int* yUnit)
{
    wxScrolledWindow* scrolledWin = (wxScrolledWindow*)self;
    if (scrolledWin && xUnit && yUnit) {
        int xppu, yppu;
        scrolledWin->GetScrollPixelsPerUnit(&xppu, &yppu);
        *xUnit = xppu;
        *yUnit = yppu;
    }
    else if (xUnit) {
        *xUnit = 0;
    }
    else if (yUnit) {
        *yUnit = 0;
    }
}

// No wxd_ScrolledWindow_Destroy needed, parent manages lifetime.
WXD_EXPORTED wxd_Point wxd_ScrolledWindow_GetViewStart(wxd_ScrolledWindow_t* self) {
    wxd_Point point{0, 0};
    if (self) reinterpret_cast<wxScrolledWindow*>(self)->GetViewStart(&point.x, &point.y);
    return point;
}

} // extern "C"
