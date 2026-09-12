import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { ChatSubagentEntry } from '@vibe/ui/components/ChatSubagentEntry';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe('delegated agent disclosure', () => {
  const render = (expanded: boolean) =>
    renderToStaticMarkup(
      createElement(ChatSubagentEntry, {
        description: 'parent-thread → /root/research · completed',
        subagentType: '/root/research',
        result: { value: 'Child answer, not the parent conclusion' },
        expanded,
        onToggle: vi.fn(),
        status: { status: 'success' },
        active: false,
        renderMarkdown: ({ content }) => createElement('div', null, content),
      })
    );

  it('shows ancestry in a keyboard-accessible collapsed card', () => {
    const html = render(false);
    expect(html).toContain('parent-thread → /root/research');
    expect(html).toContain('aria-expanded="false"');
    expect(html).toContain('tabindex="0"');
    expect(html).not.toContain('Child answer, not the parent conclusion');
  });

  it('shows the child output when expanded', () => {
    expect(render(true)).toContain('aria-expanded="true"');
    expect(render(true)).toContain('Child answer, not the parent conclusion');
  });
});
