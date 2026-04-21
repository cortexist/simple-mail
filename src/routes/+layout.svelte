<script lang="ts">
  import '../app.css';
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  onMount(() => {
    let bubble: HTMLDivElement | null = null;
    let arrow: HTMLDivElement | null = null;
    let showTimeout: ReturnType<typeof setTimeout> | null = null;
    let currentTarget: Element | null = null;

    function createElements() {
      bubble = document.createElement('div');
      bubble.className = 'tooltip-bubble';
      arrow = document.createElement('div');
      arrow.className = 'tooltip-arrow';
      document.body.appendChild(bubble);
      document.body.appendChild(arrow);
    }

    function hide() {
      if (showTimeout) { clearTimeout(showTimeout); showTimeout = null; }
      bubble?.classList.remove('visible');
      arrow?.classList.remove('visible');
      currentTarget = null;
    }

    function position(target: Element) {
      if (!bubble || !arrow) return;
      const text = target.getAttribute('data-tooltip');
      if (!text) { hide(); return; }

      const placement = target.getAttribute('data-tooltip-position') || 'bottom';
      bubble.textContent = text;
      arrow.setAttribute('data-placement', placement);

      // Measure after setting text
      const rect = target.getBoundingClientRect();
      bubble.style.left = '0';
      bubble.style.top = '0';
      bubble.classList.add('visible');
      const bRect = bubble.getBoundingClientRect();
      bubble.classList.remove('visible');

      let bx: number, by: number, ax: number, ay: number;
      const gap = 8;
      const arrowGap = 4;

      if (placement === 'top') {
        bx = rect.left + rect.width / 2 - bRect.width / 2;
        by = rect.top - gap - bRect.height;
        ax = rect.left + rect.width / 2 - 4;
        ay = rect.top - arrowGap - 8;
      } else if (placement === 'right') {
        bx = rect.right + gap;
        by = rect.top + rect.height / 2 - bRect.height / 2;
        ax = rect.right + arrowGap;
        ay = rect.top + rect.height / 2 - 4;
      } else if (placement === 'left') {
        bx = rect.left - gap - bRect.width;
        by = rect.top + rect.height / 2 - bRect.height / 2;
        ax = rect.left - arrowGap - 8;
        ay = rect.top + rect.height / 2 - 4;
      } else if (placement === 'bottom-start') {
        bx = rect.left;
        by = rect.bottom + gap;
        ax = rect.left + 10;
        ay = rect.bottom + arrowGap;
      } else if (placement === 'bottom-end') {
        bx = rect.right - bRect.width;
        by = rect.bottom + gap;
        ax = rect.right - 10 - 8;
        ay = rect.bottom + arrowGap;
      } else {
        // bottom (default)
        bx = rect.left + rect.width / 2 - bRect.width / 2;
        by = rect.bottom + gap;
        ax = rect.left + rect.width / 2 - 4;
        ay = rect.bottom + arrowGap;
      }

      // Clamp to viewport
      bx = Math.max(4, Math.min(bx, window.innerWidth - bRect.width - 4));
      by = Math.max(4, Math.min(by, window.innerHeight - bRect.height - 4));

      bubble.style.left = `${bx}px`;
      bubble.style.top = `${by}px`;
      arrow.style.left = `${ax}px`;
      arrow.style.top = `${ay}px`;

      bubble.classList.add('visible');
      arrow.classList.add('visible');
    }

    function onPointerEnter(e: Event) {
      const target = (e.target as Element)?.closest?.('[data-tooltip]');
      if (!target || target === currentTarget) return;
      if (!bubble) createElements();
      currentTarget = target;
      if (showTimeout) clearTimeout(showTimeout);
      showTimeout = setTimeout(() => position(target), 400);
    }

    function onPointerLeave(e: Event) {
      const target = (e.target as Element)?.closest?.('[data-tooltip]');
      if (target && target === currentTarget) hide();
    }

    document.addEventListener('pointerenter', onPointerEnter, true);
    document.addEventListener('pointerleave', onPointerLeave, true);
    document.addEventListener('pointerdown', hide, true);
    document.addEventListener('scroll', hide, true);

    return () => {
      document.removeEventListener('pointerenter', onPointerEnter, true);
      document.removeEventListener('pointerleave', onPointerLeave, true);
      document.removeEventListener('pointerdown', hide, true);
      document.removeEventListener('scroll', hide, true);
      bubble?.remove();
      arrow?.remove();
    };
  });
</script>

{@render children()}
