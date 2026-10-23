import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

export type MenuEntry = {
  label: string;
  shortcut?: string;
  action: () => void;
  disabled?: boolean;
  selected?: boolean;
  separator?: boolean;
};
export type AppMenu = {
  label: string;
  icon?: ReactNode;
  brand?: boolean;
  items: MenuEntry[];
};

export function AppMenus({
  menus,
  onBeforeOpen,
}: {
  menus: AppMenu[];
  onBeforeOpen: () => void;
}) {
  const [open, setOpen] = useState<number | null>(null);
  const root = useRef<HTMLElement>(null);
  const triggers = useRef<(HTMLButtonElement | null)[]>([]);
  const popup = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (open === null) return;
    const dismiss = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(null);
    };
    const focus = (e: FocusEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(null);
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("focusin", focus);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("focusin", focus);
    };
  }, [open]);
  useEffect(() => {
    if (open !== null)
      popup.current
        ?.querySelector<HTMLButtonElement>("button:not(:disabled)")
        ?.focus();
  }, [open]);
  useLayoutEffect(() => {
    if (open === null) return;
    function position() {
      const el = popup.current;
      if (!el) return;
      el.style.transform = "";
      const box = el.getBoundingClientRect();
      const shift = Math.max(
        8 - box.left,
        Math.min(0, window.innerWidth - 8 - box.right),
      );
      el.style.transform = `translateX(${shift}px)`;
    }
    position();
    window.addEventListener("resize", position);
    return () => window.removeEventListener("resize", position);
  }, [open]);
  function show(index: number) {
    onBeforeOpen();
    setOpen(index);
  }
  return (
    <nav className="application-menus" aria-label="Soliton" ref={root}>
      <div role="menubar" className="menu-triggers">
        {menus.map((menu, index) => (
          <div className="menu-anchor" key={menu.label}>
            <button
              ref={(e) => {
                triggers.current[index] = e;
              }}
              role="menuitem"
              aria-haspopup="menu"
              aria-expanded={open === index}
              aria-label={menu.label}
              className={
                "menu-trigger " +
                (menu.brand ? "brand-trigger " : "") +
                (open === index ? "active" : "")
              }
              onPointerDown={onBeforeOpen}
              onClick={() => (open === index ? setOpen(null) : show(index))}
              onPointerEnter={() => {
                if (open !== null && open !== index) show(index);
              }}
              onKeyDown={(e) => {
                if (["ArrowDown", "ArrowUp"].includes(e.key)) {
                  e.preventDefault();
                  show(index);
                }
                if (["ArrowLeft", "ArrowRight"].includes(e.key)) {
                  e.preventDefault();
                  triggers.current[
                    (index + (e.key === "ArrowRight" ? 1 : menus.length - 1)) %
                      menus.length
                  ]?.focus();
                }
              }}
            >
              {menu.icon}
              <span>{menu.label}</span>
            </button>
            {open === index && (
              <div
                className="application-menu"
                role="menu"
                aria-label={menu.label}
                ref={popup}
                onKeyDown={(e) => {
                  const items = Array.from(
                    popup.current?.querySelectorAll<HTMLButtonElement>(
                      "button:not(:disabled)",
                    ) || [],
                  );
                  const current = items.indexOf(
                    document.activeElement as HTMLButtonElement,
                  );
                  if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
                    e.preventDefault();
                    const next =
                      e.key === "Home"
                        ? 0
                        : e.key === "End"
                          ? items.length - 1
                          : (current +
                              (e.key === "ArrowDown" ? 1 : items.length - 1)) %
                            items.length;
                    items[next]?.focus();
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    e.stopPropagation();
                    setOpen(null);
                    triggers.current[index]?.focus();
                  } else if (["ArrowLeft", "ArrowRight"].includes(e.key)) {
                    e.preventDefault();
                    show(
                      (index +
                        (e.key === "ArrowRight" ? 1 : menus.length - 1)) %
                        menus.length,
                    );
                  } else if (e.key === "Tab") {
                    setOpen(null);
                    triggers.current[index]?.focus();
                  }
                }}
              >
                {menu.items.map((item, i) => (
                  <div key={i}>
                    {item.separator && (
                      <div role="separator" className="menu-divider" />
                    )}
                    <button
                      role={
                        item.selected !== undefined
                          ? "menuitemradio"
                          : "menuitem"
                      }
                      aria-label={item.label}
                      aria-checked={item.selected}
                      disabled={item.disabled}
                      onClick={() => {
                        setOpen(null);
                        triggers.current[index]?.focus();
                        item.action();
                      }}
                    >
                      <span className="menu-check">
                        {item.selected ? "✓" : ""}
                      </span>
                      <span>{item.label}</span>
                      {item.shortcut && <kbd>{item.shortcut}</kbd>}
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>
    </nav>
  );
}
