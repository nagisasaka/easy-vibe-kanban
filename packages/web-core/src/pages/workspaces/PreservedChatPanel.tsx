import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { Panel } from 'react-resizable-panels';

/** Keep the composer, timeline and their measured geometry alive during reading. */
export function PreservedChatPanel({
  visible,
  children,
}: {
  visible: boolean;
  children: ReactNode;
}) {
  const panelElementRef = useRef<HTMLDivElement>(null);
  const [lastWidth, setLastWidth] = useState(640);
  const [allocatedWidth, setAllocatedWidth] = useState(0);

  useLayoutEffect(() => {
    const element = panelElementRef.current;
    if (!element) return;
    const rememberWidth = () => {
      const width = element.clientWidth;
      setAllocatedWidth(width);
      if (visible && width > 0) setLastWidth(width);
    };
    rememberWidth();
    const observer = new ResizeObserver(rememberWidth);
    observer.observe(element);
    return () => observer.disconnect();
  }, [visible]);

  return (
    <Panel
      id="left-main"
      elementRef={panelElementRef}
      minSize="12%"
      collapsible
      collapsedSize="0%"
      className="min-w-0 h-full overflow-hidden"
    >
      <div
        className="h-full overflow-hidden"
        aria-hidden={!visible || undefined}
        {...(!visible ? { inert: '' } : {})}
        style={{
          // Reopening is asynchronous: keep the frozen geometry until the
          // Group has actually expanded, not just until visible becomes true.
          width: visible && allocatedWidth > 0 ? '100%' : lastWidth,
          visibility: visible && allocatedWidth > 0 ? 'visible' : 'hidden',
        }}
      >
        {children}
      </div>
    </Panel>
  );
}
