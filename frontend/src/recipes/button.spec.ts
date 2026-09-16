import { describe, expect, it } from 'vitest';
import { button, buttonIconSize, type ButtonSize, type ButtonVariant } from './button';

const VARIANTS: ButtonVariant[] = ['primary', 'secondary', 'danger', 'warning', 'ghost', 'ghost-danger'];
const SIZES: ButtonSize[] = ['sm', 'md', 'lg'];

describe('button recipe', () => {
  it('gives every variant a fill or a text colour and every size a text size', () => {
    for (const variant of VARIANTS) {
      expect(button({ variant })).toMatch(/\b(bg-|text-)/);
    }
    for (const size of SIZES) {
      expect(button({ size })).toMatch(/\btext-(xs|sm)\b/);
    }
  });

  it('defaults to a medium primary button', () => {
    expect(button()).toBe(button({ variant: 'primary', size: 'md' }));
  });

  it('pads a labelled button horizontally and an icon-only button squarely', () => {
    for (const size of SIZES) {
      const labelled = button({ size });
      const iconOnly = button({ size, iconOnly: true });
      expect(labelled).toMatch(/\bpx-\d/);
      expect(iconOnly).not.toMatch(/\bpx-\d/);
      expect(iconOnly).toMatch(/\bp-[\d.]+/);
      expect(iconOnly).toMatch(/pointer-coarse:min-w-/);
    }
  });

  it('carries the focus ring and never a raw palette colour', () => {
    for (const variant of VARIANTS) {
      const classes = button({ variant });
      expect(classes).toContain('focus-visible:ring-accent');
      expect(classes).not.toMatch(/\b(bg|text|border)-(red|blue|gray|slate|amber|green)-\d/);
    }
  });

  it('stretches only when asked', () => {
    expect(button()).not.toMatch(/\bw-full\b/);
    expect(button({ block: true })).toMatch(/\bw-full\b/);
  });

  it('matches icon weight to size', () => {
    expect(buttonIconSize('sm')).toBe('xs');
    expect(buttonIconSize('md')).toBe('sm');
    expect(buttonIconSize('lg')).toBe('sm');
  });
});
