"""Small native widgets used by Tracksplit's desktop interface."""
import math
import tkinter as tk
from tkinter import ttk

FONT = "Segoe UI" if __import__("sys").platform == "win32" else "DejaVu Sans"


_corner_tiles = {}


def _color(value):
    if not value:
        return (0, 0, 0)
    value = value.lstrip("#")
    return tuple(int(value[index:index+2], 16) for index in (0, 2, 4))


def _corner_photo(canvas, radius, fill, outline, background, corner):
    key = (str(canvas.tk), radius, fill, outline, background, corner)
    if key in _corner_tiles:
        return _corner_tiles[key]
    photo = tk.PhotoImage(master=canvas, width=radius, height=radius)
    background_rgb, fill_rgb, outline_rgb = _color(background), _color(fill), _color(outline)
    scale, samples = 4, 16
    for y in range(radius):
        colors = []
        for x in range(radius):
            fill_count = outline_count = 0
            for sy in range(scale):
                py = y + (sy + .5) / scale
                for sx in range(scale):
                    px = x + (sx + .5) / scale
                    dx = radius - px if corner in ("nw", "sw") else px
                    dy = radius - py if corner in ("nw", "ne") else py
                    distance = radius - math.hypot(dx, dy)
                    if distance >= 0:
                        if outline_rgb != fill_rgb and distance < 1:
                            outline_count += 1
                        else:
                            fill_count += 1
            background_count = samples - fill_count - outline_count
            rgb = tuple((background_rgb[c]*background_count + fill_rgb[c]*fill_count
                         + outline_rgb[c]*outline_count + samples//2)//samples for c in range(3))
            colors.append("#%02x%02x%02x" % rgb)
        photo.put("{" + " ".join(colors) + "}", to=(0, y))
    _corner_tiles[key] = photo
    return photo


def rounded(canvas, x, y, width, height, radius, fill, outline=""):
    """Draw an antialiased rounded rectangle with four cached Tk photo corners."""
    if width < 1 or height < 1:
        return None
    r = max(1, min(int(radius), width//2, height//2))
    background = canvas.cget("bg")
    photo_items = []
    if fill:
        canvas.create_rectangle(x+r, y, x+width-r, y+height, fill=fill, outline="", tags="shape")
        canvas.create_rectangle(x, y+r, x+r, y+height-r, fill=fill, outline="", tags="shape")
        canvas.create_rectangle(x+width-r, y+r, x+width, y+height-r,
                                fill=fill, outline="", tags="shape")
    for corner, px, py, anchor in (
        ("nw", x, y, "nw"), ("ne", x+width, y, "ne"),
        ("sw", x, y+height, "sw"), ("se", x+width, y+height, "se"),
    ):
        photo = _corner_photo(canvas, r, fill, outline or fill or background, background, corner)
        photo_items.append(canvas.create_image(px, py, image=photo, anchor=anchor, tags="shape"))
    if outline:
        canvas.create_line(x+r, y, x+width-r, y, fill=outline, tags="shape")
        canvas.create_line(x+r, y+height-1, x+width-r, y+height-1, fill=outline, tags="shape")
        canvas.create_line(x, y+r, x, y+height-r, fill=outline, tags="shape")
        canvas.create_line(x+width-1, y+r, x+width-1, y+height-r, fill=outline, tags="shape")
    canvas._rounded_images = getattr(canvas, "_rounded_images", {})
    for item in photo_items:
        canvas._rounded_images[item] = canvas.itemcget(item, "image")
    return photo_items[0] if photo_items else None


class WaveMark(tk.Canvas):
    def __init__(self, parent, *, size=40, bg, accent, ink):
        super().__init__(parent, width=size, height=size, bg=bg, highlightthickness=0)
        self.size = size
        self.set_theme(accent, ink)

    def set_theme(self, accent, ink):
        self.delete("all")
        s = self.size
        rounded(self, 0, 0, s, s, s*.3, accent)
        for i, h in enumerate((.22, .46, .64, .34)):
            x = s*(.26+i*.16)
            self.create_line(x, s*(1-h)/2, x, s*(1+h)/2,
                             fill=ink, width=max(2, s*.065), capstyle="round")


class EmptyState(tk.Canvas):
    def __init__(self, parent, title, description):
        super().__init__(parent, highlightthickness=0, width=420, height=170)
        self.title, self.description = title, description
        self.theme = None
        self.bind("<Configure>", self._draw)

    def set_theme(self, theme):
        self.theme = theme
        self.configure(bg=theme["card"])
        self._draw()

    def _draw(self, _event=None):
        if not self.theme:
            return
        self.delete("all")
        t = self.theme
        w, h = self.winfo_width(), self.winfo_height()
        if h < 100:
            self.create_text(w/2, h/2, text=self.title, fill=t["muted"], font=(FONT, 9),
                             width=max(80, w-20), justify="center")
            return
        cy = max(22, h / 2 - 36)
        for i in range(23):
            x = w/2 + (i-11)*7
            length = 5 + 29 * abs(math.sin(i*.72)) * math.sin(math.pi*(i+1)/24)
            self.create_line(x, cy-length/2, x, cy+length/2,
                             width=3, fill=t["accent"] if 7 <= i <= 15 else t["border"], capstyle="round")
        self.create_text(w/2, cy+36, text=self.title, fill=t["ink"], font=(FONT, 12, "bold"),
                              width=max(100, w-32), justify="center")
        if h >= 135:
            self.create_text(w/2, cy+64, text=self.description, fill=t["muted"],
                             font=(FONT, 9), width=max(80, w-24), justify="center")


class MediaList(ttk.Treeview):
    """Treeview keyboard navigation, with explicit per-row selection marks."""
    def __init__(self, parent, *, columns, empty_title, empty_description):
        super().__init__(parent, columns=columns, show="tree headings", selectmode="extended", height=3)
        self.column("#0", width=68, minwidth=68, stretch=False, anchor="center")
        self.heading("#0", text="№", anchor="center")
        self.empty = EmptyState(parent, empty_title, empty_description)
        self.bind("<<TreeviewSelect>>", self._selection_changed, add=True)
        self.bind("<Button-1>", self._click)
        self.bind("<space>", self._space)
        self.bind("<Control-a>", self._select_all)
        self.bind("<Configure>", self._update_empty, add=True)
        self._update_empty()

    def _update_empty(self, _event=None):
        if self.get_children():
            self.empty.place_forget()
        else:
            self.empty.place(x=0, y=38, relwidth=1, relheight=1, height=-38)
            tk.Misc.lift(self.empty)

    def insert(self, parent, index, iid=None, **kw):
        count = len(self.get_children())
        kw.setdefault("text", f"○  {count+1:02}")
        kw.setdefault("tags", ("even" if count % 2 == 0 else "odd",))
        result = super().insert(parent, index, iid=iid, **kw)
        self._update_empty()
        return result

    def delete(self, *items):
        super().delete(*items)
        self._update_empty()

    def _selection_changed(self, _event=None):
        selected = set(self.selection())
        for index, item in enumerate(self.get_children(), 1):
            self.item(item, text=f"{'✓' if item in selected else '○'}  {index:02}")

    def _click(self, event):
        item = self.identify_row(event.y)
        if item and self.identify_region(event.x, event.y) in ("tree", "cell") and not event.state & 1:
            self.focus_set()
            self.focus(item)
            self.selection_toggle(item)
            return "break"

    def _space(self, _event=None):
        if self.focus():
            self.selection_toggle(self.focus())
        return "break"

    def _select_all(self, _event=None):
        self.selection_set(self.get_children())
        return "break"

    def set_theme(self, theme):
        self.tag_configure("even", background=theme["card"])
        self.tag_configure("odd", background=theme["heading"])
        self.empty.set_theme(theme)


class Choice(tk.Canvas):
    """Rounded choice field with a popup styled to match the selected theme."""
    def __init__(self, parent, *, state="readonly", width=20, values=()):
        super().__init__(parent, width=width*8+28, height=38, highlightthickness=0, takefocus=1)
        self.values = tuple(values)
        self.value = ""
        self.theme = None
        self.popup = None
        self._popup_index = 0
        self.bind("<Configure>", self._draw)
        self.bind("<FocusIn>", self._draw)
        self.bind("<FocusOut>", self._draw)
        self.bind("<Button-1>", self._open)
        self.bind("<Return>", self._open)
        self.bind("<space>", self._open)
        self.bind("<Down>", lambda _e: self._step(1))
        self.bind("<Up>", lambda _e: self._step(-1))
        self.configure(cursor="hand2")

    def get(self):
        return self.value

    def set(self, value):
        self.value = value
        self._draw()

    def _choose(self, value):
        self._close_popup()
        self.set(value)
        self.event_generate("<<ComboboxSelected>>")

    def _step(self, amount):
        if self.values:
            index = self.values.index(self.value) if self.value in self.values else 0
            self._choose(self.values[(index+amount) % len(self.values)])
        return "break"

    def _open(self, _event=None):
        if self.popup is not None and self.popup.winfo_exists():
            self._close_popup()
            return "break"
        if not self.values or not self.theme:
            return "break"
        self._popup_index = self.values.index(self.value) if self.value in self.values else 0
        theme = self.theme
        width = max(154, self.winfo_width())
        row_height, pad = 36, 7
        height = row_height*len(self.values) + pad*2
        left = min(self.winfo_rootx(), self.winfo_screenwidth()-width-8)
        top = self.winfo_rooty()+self.winfo_height()+5
        if top+height > self.winfo_screenheight()-8:
            top = max(8, self.winfo_rooty()-height-5)
        popup = tk.Toplevel(self)
        self.popup = popup
        popup.overrideredirect(True)
        popup.transient(self.winfo_toplevel())
        popup.configure(bg=theme["bg"])
        popup.geometry(f"{width}x{height}+{left}+{top}")
        popup_canvas = tk.Canvas(popup, width=width, height=height, bg=theme["bg"],
                                 highlightthickness=0, takefocus=0)
        popup_canvas.pack(fill="both", expand=True)
        self._popup_canvas = popup_canvas
        self._draw_popup_rows()
        popup.bind("<Escape>", lambda _e: self._close_popup())
        popup.bind("<Up>", lambda _e: self._move_popup(-1))
        popup.bind("<Down>", lambda _e: self._move_popup(1))
        popup.bind("<Return>", lambda _e: self._choose(self.values[self._popup_index]))
        popup.bind("<FocusOut>", self._popup_focus_out, add=True)
        popup.after_idle(popup.focus_force)
        return "break"

    def _draw_popup_rows(self):
        if self.popup is None or not self.popup.winfo_exists():
            return
        canvas = self._popup_canvas
        theme = self.theme
        width = canvas.winfo_width()
        if width <= 1:
            width = int(self.popup.winfo_width())
        height = int(self.popup.winfo_height())
        canvas.delete("all")
        # Keep the popup's unselected surface on the same color as the app
        # background. A separate white card layer left a visible strip at its edge
        # on the light theme.
        rounded(canvas, 0, 0, width, height, 14, theme["bg"], theme["border"])
        row_height = 36
        for index, value in enumerate(self.values):
            top = 7 + index*row_height
            active = index == self._popup_index or value == self.value
            option_tag = f"option-{index}"
            background_tag = f"popup-bg-{index}"
            # This same-color rectangle provides a full-row hit target, including
            # the padding around the label. The selected rounded shape sits above it.
            canvas.create_rectangle(6, top, width-6, top+row_height,
                                    fill=theme["bg"], outline="", tags=(option_tag,))
            first_item = len(canvas.find_all())
            rounded(canvas, 6, top+1, width-12, row_height-2, 9, theme["selected"])
            shape_items = canvas.find_all()[first_item:]
            for item in shape_items:
                canvas.addtag_withtag(background_tag, item)
                canvas.addtag_withtag(option_tag, item)
            if not active:
                canvas.itemconfigure(background_tag, state="hidden")
            text_item = canvas.create_text(17, top+row_height/2, text=value, anchor="w",
                               fill=theme["selected_fg"] if active else theme["ink"],
                               font=(FONT, 10, "bold" if value == self.value else "normal"),
                               tags=("popup-row", option_tag, f"popup-text-{index}"))
            canvas.tag_bind(option_tag, "<Enter>",
                            lambda _e, i=index: self._highlight_popup_row(i))
            canvas.tag_bind(option_tag, "<Button-1>",
                            lambda _e, v=value: self._choose(v))

    def _highlight_popup_row(self, index):
        if index == self._popup_index:
            return
        self._popup_index = index
        if self.popup is None or not self.popup.winfo_exists():
            return
        canvas = self._popup_canvas
        for row, value in enumerate(self.values):
            active = row == index
            canvas.itemconfigure(f"popup-bg-{row}", state="normal" if active else "hidden")
            canvas.itemconfigure(f"popup-text-{row}",
                                 fill=self.theme["selected_fg"] if active else self.theme["ink"],
                                 font=(FONT, 10, "bold" if value == self.value else "normal"))

    def _move_popup(self, step):
        self._popup_index = (self._popup_index+step) % len(self.values)
        self._highlight_popup_row(self._popup_index)
        return "break"

    def _popup_focus_out(self, _event=None):
        if self.popup is not None:
            self.popup.after_idle(self._close_popup_if_unfocused)

    def _close_popup_if_unfocused(self):
        if self.popup is None or not self.popup.winfo_exists():
            return
        focus = str(self.tk.call("focus"))
        if not focus.startswith(str(self.popup)):
            self._close_popup()

    def _close_popup(self):
        popup, self.popup = self.popup, None
        if popup is not None:
            try:
                popup.destroy()
            except tk.TclError:
                pass

    def set_theme(self, theme):
        self.theme = theme
        self.configure(bg=self.master.cget("bg"))
        self._draw()
        self._draw_popup_rows()

    def _draw(self, _event=None):
        if not self.theme:
            return
        self.delete("all")
        theme = self.theme
        width = self.winfo_width() if self.winfo_width() > 1 else int(self.cget("width"))
        rounded(self, 0, 0, width, 38, 14, theme["field"], theme["border"])
        self.create_text(13, 19, text=self.value, anchor="w", fill=theme["ink"], font=(FONT, 10))
        self.create_line(width-23, 17, width-19, 21, width-15, 17, fill=theme["muted"], width=1.5)
