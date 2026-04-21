<script lang="ts">
  import { untrack } from 'svelte';

  interface Props {
    /** 'prompt' shows a text input, 'confirm' shows a message with OK/Cancel */
    kind: 'prompt' | 'confirm';
    title: string;
    message?: string;
    placeholder?: string;
    initialValue?: string;
    confirmLabel?: string;
    dangerConfirm?: boolean;
    onSubmit: (value: string) => void;
    onCancel: () => void;
  }

  let {
    kind,
    title,
    message = '',
    placeholder = '',
    initialValue = '',
    confirmLabel = 'OK',
    dangerConfirm = false,
    onSubmit,
    onCancel,
  }: Props = $props();

  let inputValue = $state('');
  let inputEl: HTMLInputElement | undefined = $state();
  let overlayEl: HTMLDivElement | undefined = $state();

  // Set initial value once on mount; untrack prevents re-runs.
  $effect(() => {
    inputValue = untrack(() => initialValue);
  });

  function handleKeydown(ev: KeyboardEvent) {
    if (ev.key === 'Escape') {
      onCancel();
    } else if (ev.key === 'Enter') {
      handleSubmit();
    }
  }

  function handleSubmit() {
    if (kind === 'prompt') {
      const trimmed = inputValue.trim();
      if (!trimmed) return;
      onSubmit(trimmed);
    } else {
      onSubmit('');
    }
  }

  $effect(() => {
    if (kind === 'prompt') {
      inputEl?.focus();
      inputEl?.select();
    } else {
      overlayEl?.focus();
    }
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="modal-overlay"
  role="dialog"
  aria-modal="true"
  aria-label={title}
  tabindex="-1"
  bind:this={overlayEl}
  onkeydown={handleKeydown}
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="modal-backdrop" onclick={onCancel} onkeydown={handleKeydown}></div>
  <div class="modal-card">
    <h3 class="modal-title">{title}</h3>

    {#if kind === 'prompt'}
      {#if message}
        <p class="modal-message">{message}</p>
      {/if}
      <input
        bind:this={inputEl}
        bind:value={inputValue}
        class="modal-input"
        type="text"
        placeholder={placeholder}
      />
    {:else}
      <p class="modal-message">{message}</p>
    {/if}

    <div class="modal-actions">
      <button class="modal-btn modal-btn-cancel" onclick={onCancel}>Cancel</button>
      <button
        class="modal-btn modal-btn-confirm"
        class:danger={dangerConfirm}
        onclick={handleSubmit}
        disabled={kind === 'prompt' && !inputValue.trim()}
      >
        {confirmLabel}
      </button>
    </div>
  </div>
</div>

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 2000;
  }

  .modal-backdrop {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
  }

  .modal-card {
    position: relative;
    background: var(--bg-primary);
    border: 1px solid var(--border-light);
    border-radius: 8px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
    padding: 20px 24px;
    min-width: 340px;
    max-width: 420px;
    width: 90%;
  }

  .modal-title {
    margin: 0 0 12px 0;
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .modal-message {
    margin: 0 0 16px 0;
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.5;
  }

  .modal-input {
    width: 100%;
    padding: 7px 10px;
    font-size: 13px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    outline: none;
    margin-bottom: 16px;
    box-sizing: border-box;
  }

  .modal-input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .modal-btn {
    padding: 6px 16px;
    font-size: 13px;
    font-weight: 500;
    border-radius: 4px;
    cursor: pointer;
    transition: background 0.1s, color 0.1s;
  }

  .modal-btn-cancel {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--border);
  }

  .modal-btn-cancel:hover {
    background: var(--bg-hover);
  }

  .modal-btn-confirm {
    background: var(--accent);
    color: var(--text-on-accent);
    border: 1px solid transparent;
  }

  .modal-btn-confirm:hover {
    background: var(--accent-hover);
  }

  .modal-btn-confirm:active {
    background: var(--accent-active);
  }

  .modal-btn-confirm:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .modal-btn-confirm.danger {
    background: var(--danger);
  }

  .modal-btn-confirm.danger:hover {
    background: #c42b2f;
  }

  :global([data-theme="dark"]) .modal-btn-confirm.danger:hover {
    background: #e03e42;
  }
</style>
