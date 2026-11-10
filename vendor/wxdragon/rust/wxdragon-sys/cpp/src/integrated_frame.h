// Opt-in client-area title bar. Other wxFrames keep their original behaviour.
// Retain WS_CAPTION/WS_THICKFRAME, so the OS owns move, resize, system menu,
// caption commands, taskbar and Windows 11 snap layouts.
#pragma once
#include <wx/frame.h>
#ifdef __WXMSW__
#include <wx/weakref.h>
#include <commctrl.h>
#include <dwmapi.h>
#include <vector>

class WxdIntegratedFrame : public wxFrame {
    struct Region {
        wxWeakRef<wxWindow> window;
        int role; // 1: drag region; 2: native maximize/snap target
    };
    struct Hook { wxWeakRef<WxdIntegratedFrame> frame; };
    bool integrated = false;
    bool dark = false;
    COLORREF borderColour = 0;
    std::vector<Region> regions;
    std::vector<wxWeakRef<wxWindow>> hookedWindows;
    wxWeakRef<wxWindow> hoveredMax;
    static constexpr UINT_PTR HookId = 0x54434843;

    static LRESULT CALLBACK ChildProc(HWND hwnd, UINT message, WPARAM wp, LPARAM lp,
                                      UINT_PTR, DWORD_PTR ref) {
        auto* hook = reinterpret_cast<Hook*>(ref);
        if (message == WM_NCDESTROY) {
            RemoveWindowSubclass(hwnd, ChildProc, HookId);
            delete hook;
            return DefSubclassProc(hwnd, message, wp, lp);
        }
        if (message == WM_NCHITTEST && hook->frame) {
            const auto hit = hook->frame->ChromeHitTest(lp);
            // Let Windows continue through the child HWNDs to the main frame.
            // Interactive service/account controls still return HTCLIENT.
            if (hit != HTCLIENT && hit != HTNOWHERE) return HTTRANSPARENT;
        }
        return DefSubclassProc(hwnd, message, wp, lp);
    }
    static wxWindow* LeafAt(wxWindow* window, const wxPoint& point) {
        for (auto node = window->GetChildren().GetLast(); node; node = node->GetPrevious()) {
            wxWindow* child = node->GetData();
            if (child->IsShown() && child->GetScreenRect().Contains(point))
                return LeafAt(child, point);
        }
        return window;
    }
    void ApplyDwm() {
        // Optional DWM attributes, with no new platform runtime dependency.
        static HMODULE dwm = LoadLibraryW(L"dwmapi.dll");
        if (!dwm) return;
        using SetAttribute = HRESULT (WINAPI*)(HWND, DWORD, LPCVOID, DWORD);
        using Extend = HRESULT (WINAPI*)(HWND, const MARGINS*);
        auto set = reinterpret_cast<SetAttribute>(GetProcAddress(dwm, "DwmSetWindowAttribute"));
        if (set) {
            const BOOL value = dark;
            const int rounded = 2; // DWMWCP_ROUND; ignored on older Windows.
            set(GetHwnd(), 20, &value, sizeof(value));
            set(GetHwnd(), 33, &rounded, sizeof(rounded));
            set(GetHwnd(), 34, &borderColour, sizeof(borderColour));
        }
        auto extend = reinterpret_cast<Extend>(GetProcAddress(dwm, "DwmExtendFrameIntoClientArea"));
        if (extend) {
            const MARGINS margins = {0, 0, 1, 0};
            extend(GetHwnd(), &margins);
        }
    }
    void MaxHover(wxWindow* target) {
        if (hoveredMax == target) return;
        if (hoveredMax) {
            wxMouseEvent leave(wxEVT_LEAVE_WINDOW);
            leave.SetEventObject(hoveredMax.get());
            hoveredMax->GetEventHandler()->ProcessEvent(leave);
        }
        hoveredMax = target;
        if (target) {
            wxMouseEvent enter(wxEVT_ENTER_WINDOW);
            enter.SetEventObject(target);
            target->GetEventHandler()->ProcessEvent(enter);
        }
    }
public:
    using wxFrame::wxFrame;
    ~WxdIntegratedFrame() override {
        integrated = false;
        hoveredMax = nullptr;
        // Detach before wxFrame destroys children, while our members are alive.
        // Already-destroyed children have removed their own hook and weak ref.
        for (const auto& window : hookedWindows) {
            if (!window) continue;
            DWORD_PTR data = 0;
            if (GetWindowSubclass(window->GetHWND(), ChildProc, HookId, &data)) {
                RemoveWindowSubclass(window->GetHWND(), ChildProc, HookId);
                delete reinterpret_cast<Hook*>(data);
            }
        }
    }
    bool EnableIntegrated(bool isDark, uint32_t colour) {
        dark = isDark;
        borderColour = colour;
        integrated = true;
        ApplyDwm();
        SetWindowPos(GetHwnd(), nullptr, 0, 0, 0, 0,
                     SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        return true;
    }
    void RegisterRegion(wxWindow* window, int role) {
        if (!integrated || !window || (role != 1 && role != 2)) return;
        // Reject a window owned by a different top level frame.
        wxWindow* top = window;
        while (top && top != this) top = top->GetParent();
        if (top != this) return;
        regions.push_back({window, role});
        for (wxWindow* child = window; child && child != this; child = child->GetParent()) {
            DWORD_PTR existing = 0;
            if (GetWindowSubclass(child->GetHWND(), ChildProc, HookId, &existing)) continue;
            auto* hook = new Hook{this};
            if (SetWindowSubclass(child->GetHWND(), ChildProc, HookId,
                                  reinterpret_cast<DWORD_PTR>(hook))) hookedWindows.push_back(child);
            else delete hook;
        }
    }
    LRESULT ChromeHitTest(LPARAM lp) const {
        if (!integrated) return HTCLIENT;
        const wxPoint p(static_cast<short>(LOWORD(lp)), static_cast<short>(HIWORD(lp)));
        const wxRect rect = GetScreenRect();
        if (!rect.Contains(p)) return HTNOWHERE;
        // Real system metrics follow the current monitor DPI, including the
        // invisible native border. The top edge extends into the title bar.
        const int edge = FromDIP(6);
        if (!IsMaximized()) {
            const bool left = p.x < rect.x + edge, right = p.x >= rect.GetRight() - edge;
            const bool top = p.y < rect.y + edge, bottom = p.y >= rect.GetBottom() - edge;
            if (top) return left ? HTTOPLEFT : right ? HTTOPRIGHT : HTTOP;
            if (bottom) return left ? HTBOTTOMLEFT : right ? HTBOTTOMRIGHT : HTBOTTOM;
            if (left) return HTLEFT;
            if (right) return HTRIGHT;
        }
        for (const auto& region : regions) {
            wxWindow* w = region.window.get();
            if (!w || !w->IsShown() || !w->GetScreenRect().Contains(p)) continue;
            if (region.role == 2) return HTMAXBUTTON;
            wxWindow* leaf = LeafAt(w, p);
            if (leaf == w) return HTCAPTION;
            for (const auto& other : regions)
                if (other.role == 1 && other.window.get() == leaf) return HTCAPTION;
        }
        return HTCLIENT;
    }
    WXLRESULT MSWWindowProc(WXUINT message, WXWPARAM wp, WXLPARAM lp) override {
        if (integrated) {
            switch (message) {
            case WM_NCCALCSIZE:
                if (wp) {
                    auto* params = reinterpret_cast<NCCALCSIZE_PARAMS*>(lp);
                    const LONG top = params->rgrc[0].top;
                    wxFrame::MSWWindowProc(message, wp, lp);
                    // Keep the native side/bottom insets and maximized work
                    // area; only replace the standard caption with our UI.
                    params->rgrc[0].top = top + (IsZoomed(GetHwnd())
                        ? GetSystemMetricsForDpi(SM_CYSIZEFRAME, GetDPI().y)
                          + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, GetDPI().y) : 0);
                    return 0;
                }
                break;
            case WM_NCHITTEST: {
                const LRESULT hit = ChromeHitTest(lp);
                if (hit != HTCLIENT) return hit;
                // There are no standard DWM caption buttons to hit-test: the
                // custom maximize region above supplies HTMAXBUTTON instead.
                return HTCLIENT;
            }
            case WM_NCMOUSEMOVE: {
                wxWindow* target = nullptr;
                if (wp == HTMAXBUTTON) {
                    for (const auto& region : regions)
                        if (region.role == 2 && region.window) { target = region.window.get(); break; }
                    TRACKMOUSEEVENT track{sizeof(track), TME_LEAVE | TME_NONCLIENT, GetHwnd(), 0};
                    TrackMouseEvent(&track);
                }
                MaxHover(target);
                break; // DefWindowProc retains the Windows 11 snap flyout.
            }
            case WM_NCMOUSELEAVE:
                MaxHover(nullptr);
                break;
            case WM_DWMCOMPOSITIONCHANGED:
                ApplyDwm();
                break;
            }
        }
        return wxFrame::MSWWindowProc(message, wp, lp);
    }
};
#else
// Keep native macOS window controls, traffic lights and system integration.
using WxdIntegratedFrame = wxFrame;
#endif
