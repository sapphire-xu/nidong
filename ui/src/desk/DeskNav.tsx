import { useEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { Tooltip } from "@fluentui/react-components";
import {
  PanelLeftContract20Regular,
  PanelLeftExpand20Regular,
  PinOff16Regular,
  Share16Regular,
} from "@fluentui/react-icons";
import { computeDragTargetIndex, type NavId, type ToolNavId } from "./navModel";

export type DeskNavItem = {
  id: NavId;
  label: string;
  icon: JSX.Element;
  activeIcon: JSX.Element;
  pinned?: boolean;
  badge?: string;
};

type DragRenderState = {
  id: NavId;
  fromIndex: number;
  overIndex: number;
  dy: number;
  pitch: number;
};

type DragSession = {
  id: NavId;
  fromIndex: number;
  pointerId: number;
  rowEl: HTMLDivElement;
  startY: number;
  startScrollTop: number;
  centers: number[];
  pitch: number;
  minDy: number;
  maxDy: number;
  active: boolean;
  overIndex: number;
};

const DRAG_THRESHOLD_PX = 6;

export function DeskNav(props: {
  items: DeskNavItem[];
  selectedId: NavId;
  collapsed: boolean;
  shareOn: boolean;
  onToggleCollapse: () => void;
  onSelect: (id: NavId) => void;
  onReorder: (fromIndex: number, toIndex: number) => void;
  onUnpin: (id: ToolNavId) => void;
  onOpenShare: () => void;
}) {
  const {
    items,
    selectedId,
    collapsed,
    shareOn,
    onToggleCollapse,
    onSelect,
    onReorder,
    onUnpin,
    onOpenShare,
  } = props;

  const listRef = useRef<HTMLElement>(null);
  const sessionRef = useRef<DragSession | null>(null);
  const suppressClickRef = useRef(false);
  const [drag, setDrag] = useState<DragRenderState | null>(null);

  useEffect(() => {
    if (!drag) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const s = sessionRef.current;
      if (s) {
        try {
          s.rowEl.releasePointerCapture(s.pointerId);
        } catch {
          // Ignore if already released.
        }
        sessionRef.current = null;
      }
      suppressClickRef.current = false;
      setDrag(null);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [drag]);

  function handlePointerDown(e: ReactPointerEvent<HTMLDivElement>, index: number, id: NavId) {
    if (e.button !== 0 || items.length < 2) return;
    if ((e.target as HTMLElement | null)?.closest("[data-no-drag='true']")) return;
    const listEl = listRef.current;
    if (!listEl) return;

    const nodes = Array.from(listEl.querySelectorAll<HTMLElement>("[data-nav-id]"));
    if (nodes.length !== items.length) return;

    const listRect = listEl.getBoundingClientRect();
    const scrollTop = listEl.scrollTop;
    const rects = nodes.map((node) => {
      const r = node.getBoundingClientRect();
      return {
        top: r.top - listRect.top + scrollTop,
        height: r.height,
        center: r.top - listRect.top + scrollTop + r.height / 2,
      };
    });

    const centers = rects.map((r) => r.center);
    const pitch =
      rects.length > 1
        ? Math.max(24, rects[1].top - rects[0].top)
        : Math.max(24, rects[0].height + 4);
    const currentTop = rects[index].top;
    const firstTop = rects[0].top;
    const lastTop = rects[rects.length - 1].top;

    sessionRef.current = {
      id,
      fromIndex: index,
      pointerId: e.pointerId,
      rowEl: e.currentTarget,
      startY: e.clientY,
      startScrollTop: scrollTop,
      centers,
      pitch,
      minDy: firstTop - currentTop - 10,
      maxDy: lastTop - currentTop + 10,
      active: false,
      overIndex: index,
    };
  }

  function handlePointerMove(e: ReactPointerEvent<HTMLDivElement>) {
    const s = sessionRef.current;
    if (!s || e.pointerId !== s.pointerId) return;
    const listEl = listRef.current;
    if (!listEl) return;

    if (s.active) {
      const listRect = listEl.getBoundingClientRect();
      const edge = 28;
      if (e.clientY < listRect.top + edge && listEl.scrollTop > 0) {
        listEl.scrollTop = Math.max(0, listEl.scrollTop - 8);
      } else if (
        e.clientY > listRect.bottom - edge &&
        listEl.scrollTop < listEl.scrollHeight - listEl.clientHeight
      ) {
        listEl.scrollTop = Math.min(listEl.scrollHeight - listEl.clientHeight, listEl.scrollTop + 8);
      }
    }

    const rawDy = e.clientY - s.startY + (listEl.scrollTop - s.startScrollTop);
    if (!s.active) {
      if (Math.abs(rawDy) < DRAG_THRESHOLD_PX) return;
      s.active = true;
      suppressClickRef.current = true;
      try {
        s.rowEl.setPointerCapture(s.pointerId);
      } catch {
        // Ignore pointer capture errors on synthetic events.
      }
    }

    const clampedDy = Math.min(s.maxDy, Math.max(s.minDy, rawDy));
    const overIndex = computeDragTargetIndex(s.centers, s.fromIndex, clampedDy);
    s.overIndex = overIndex;
    setDrag({
      id: s.id,
      fromIndex: s.fromIndex,
      overIndex,
      dy: clampedDy,
      pitch: s.pitch,
    });
  }

  function finishDrag(e: ReactPointerEvent<HTMLDivElement>, commit: boolean) {
    const s = sessionRef.current;
    if (!s || e.pointerId !== s.pointerId) return;
    sessionRef.current = null;
    if (s.active) {
      try {
        if (s.rowEl.hasPointerCapture(s.pointerId)) {
          s.rowEl.releasePointerCapture(s.pointerId);
        }
      } catch {
        // Ignore release errors.
      }
      if (commit && s.overIndex !== s.fromIndex) {
        onReorder(s.fromIndex, s.overIndex);
      }
      setDrag(null);
      window.setTimeout(() => {
        suppressClickRef.current = false;
      }, 0);
    }
  }

  const sharePillButton = (
    <button
      type="button"
      className="share-pill"
      aria-label="文件分享服务已启用，点击查看"
      onClick={onOpenShare}
    >
      <span className="share-pill-ico" aria-hidden="true">
        <Share16Regular />
        <span className="share-pill-dot" />
      </span>
      <span className="share-pill-text">文件分享服务已启用</span>
    </button>
  );

  return (
    <aside
      className={`desk-nav${collapsed ? " collapsed" : ""}${drag ? " is-dragging" : ""}`}
      aria-label="侧边导航栏"
    >
      <div className="desk-nav-head">
        <Tooltip
          content={collapsed ? "展开导航栏" : "收起导航栏"}
          relationship="label"
          positioning="after"
          withArrow
        >
          <button
            type="button"
            className="nav-toggle-btn"
            aria-label={collapsed ? "展开导航栏" : "收起导航栏"}
            aria-expanded={!collapsed}
            onClick={onToggleCollapse}
          >
            {collapsed ? <PanelLeftExpand20Regular /> : <PanelLeftContract20Regular />}
          </button>
        </Tooltip>
      </div>

      <nav ref={listRef} className="desk-nav-list" aria-label="页面导航">
        {items.map((item, index) => {
          const active = item.id === selectedId;
          const isDragged = drag?.id === item.id;
          let shiftY = 0;
          if (drag && !isDragged) {
            if (drag.overIndex > drag.fromIndex && index > drag.fromIndex && index <= drag.overIndex) {
              shiftY = -drag.pitch;
            } else if (
              drag.overIndex < drag.fromIndex &&
              index >= drag.overIndex &&
              index < drag.fromIndex
            ) {
              shiftY = drag.pitch;
            }
          }

          const rowStyle: CSSProperties | undefined = isDragged
            ? { transform: `translate3d(0, ${drag.dy}px, 0) scale(1.015)` }
            : drag
              ? { transform: shiftY !== 0 ? `translate3d(0, ${shiftY}px, 0)` : "translate3d(0, 0, 0)" }
              : undefined;

          const rowClass = [
            "nav-item-row",
            active ? "active" : "",
            item.pinned ? "pinned" : "",
            isDragged ? "dragging" : "",
            drag && !isDragged ? "shifting" : "",
          ]
            .filter(Boolean)
            .join(" ");

          const tooltipText = item.badge ? `${item.label} · ${item.badge}` : item.label;

          const hitButton = (
            <button
              type="button"
              className="nav-item-hit"
              aria-current={active ? "page" : undefined}
              aria-label={collapsed ? tooltipText : undefined}
              onClick={() => {
                if (suppressClickRef.current) {
                  suppressClickRef.current = false;
                  return;
                }
                onSelect(item.id);
              }}
              onKeyDown={(e) => {
                if (e.altKey && e.key === "ArrowUp" && index > 0) {
                  e.preventDefault();
                  onReorder(index, index - 1);
                } else if (e.altKey && e.key === "ArrowDown" && index < items.length - 1) {
                  e.preventDefault();
                  onReorder(index, index + 1);
                }
              }}
            >
              <span className="nav-item-icon" aria-hidden="true">
                {active ? item.activeIcon : item.icon}
                {item.badge ? <span className="nav-item-dot" /> : null}
              </span>
              <span className="nav-item-label">{item.label}</span>
              {item.badge ? <span className="nav-item-badge">{item.badge}</span> : null}
            </button>
          );

          return (
            <div
              key={item.id}
              data-nav-id={item.id}
              className={rowClass}
              style={rowStyle}
              onPointerDown={(e) => handlePointerDown(e, index, item.id)}
              onPointerMove={handlePointerMove}
              onPointerUp={(e) => finishDrag(e, true)}
              onPointerCancel={(e) => finishDrag(e, false)}
            >
              {collapsed ? (
                <Tooltip content={tooltipText} relationship="label" positioning="after" withArrow>
                  {hitButton}
                </Tooltip>
              ) : (
                hitButton
              )}
              {item.pinned && !collapsed ? (
                <Tooltip content="取消固定" relationship="label" positioning="after" withArrow>
                  <button
                    type="button"
                    className="nav-item-unpin"
                    data-no-drag="true"
                    aria-label={`从导航栏取消固定${item.label}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      onUnpin(item.id as ToolNavId);
                    }}
                  >
                    <PinOff16Regular />
                  </button>
                </Tooltip>
              ) : null}
            </div>
          );
        })}
      </nav>

      {shareOn ? (
        <div className="desk-nav-footer">
          {collapsed ? (
            <Tooltip
              content="文件分享服务已启用 · 点击查看"
              relationship="label"
              positioning="after"
              withArrow
            >
              {sharePillButton}
            </Tooltip>
          ) : (
            sharePillButton
          )}
        </div>
      ) : null}
    </aside>
  );
}
