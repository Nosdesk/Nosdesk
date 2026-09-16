import { describe, expect, it } from 'vitest';
import { menuItem, type MenuItemTone } from './menu';

const TONES: MenuItemTone[] = ['default', 'danger', 'active'];

describe('menu row recipe', () => {
  it('keeps every row full width with the touch floor and the dense desktop tier', () => {
    for (const tone of TONES) {
      const c = menuItem({ tone });
      expect(c).toContain('w-full');
      expect(c).toContain('min-h-[44px]');
      expect(c).toContain('md:min-h-0');
      expect(c).toContain('md:text-xs');
    }
  });

  it('shows the hover wash only when the row is not keyboard-highlighted', () => {
    expect(menuItem()).toMatch(/hover:bg-surface-hover/);
    expect(menuItem({ highlighted: true })).not.toMatch(/hover:bg-/);
    expect(menuItem({ highlighted: true })).toContain('bg-accent/10');
    expect(menuItem({ tone: 'danger' })).toContain('hover:bg-status-error/10');
  });

  it('colours the row by tone', () => {
    expect(menuItem({ tone: 'danger' })).toContain('text-status-error');
    expect(menuItem({ tone: 'active' })).toContain('text-accent');
    expect(menuItem({ tone: 'default' })).toContain('text-secondary');
  });

  it('aligns two-line rows to the top on request', () => {
    expect(menuItem()).toContain('items-center');
    expect(menuItem({ align: 'start' })).toContain('items-start');
  });
});
