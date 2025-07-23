<script lang="ts">
  import type { BuiltinFnDef } from './types';

  export let name: string;
  export let def: BuiltinFnDef;
</script>

<div class="root">
  <h3 class="fn-name" id={name}>
    <a href={`#${name}`} class="anchor-link" aria-label="Link to this function">{name}</a>
  </h3>
  {#each def.signatures as sig, i}
    <div class="fn-signature">
      <div class="signature-line">
        <span class="sig-name">{name}</span>
        <span>(</span>
        {#each sig.arg_defs as arg, j}
          <span class="arg">
            <span class="arg-name">{arg.name || '...'}</span>
            {#if arg.name}<span>{': '}</span>{/if}
            <span class="arg-types">{arg.valid_types.join(' | ')}</span>
            {#if arg.default_value !== 'Required'}
              <span class="arg-default">{arg.default_value.Optional[0]}</span>
            {/if}
          </span>
          {#if j < sig.arg_defs.length - 1}{', '}
          {/if}
        {/each}
        <span>)</span>
        <span>{': '}</span>
        {#each sig.return_type as ty, k}
          <span class="return-type">{ty}</span>
          {#if k < sig.return_type.length - 1}
            {'|'}
          {/if}
        {/each}
      </div>
      {#if sig.description}
        <div class="sig-description">{sig.description}</div>
      {/if}
      {#if sig.arg_defs.length > 0}
        <div class="args-list">
          <div class="args-title">Arguments:</div>
          <ul>
            {#each sig.arg_defs as arg}
              <li>
                <div class="arg-def">
                  {#if arg.name}
                    <span class="arg-name">{arg.name}</span>
                    <span class="arg-types" style="margin-left: -9px">: {arg.valid_types.join(' | ')}</span>
                    {#if arg.default_value !== 'Required'}
                      <span class="arg-default" style="margin-left: -8px">
                        {arg.default_value.Optional[0]}
                      </span>
                    {/if}
                    {#if arg.description}<span class="arg-desc">- {arg.description}</span>{/if}
                  {:else}
                    <span class="arg-name">...</span>
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>
  {/each}
</div>

<style lang="css">
  .root {
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    display: block;
    color: #e0e0e0;
    padding: 4px 24px 16px 24px;
  }

  .fn-name {
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    font-size: 28px;
    font-weight: 600;
    margin-bottom: 18px;
    margin-top: 0;
    position: relative;
    scroll-margin-top: 70px;
  }

  .anchor-link {
    margin-right: 8px;
    color: #e0e0e0;
  }

  .fn-name:hover .anchor-link {
    opacity: 1;
    color: #cfcfcf;
  }

  .fn-signature {
    margin-bottom: 15px;
    margin-left: 20px;
    padding: 12px 16px;
    background: #232323;
    border: 1px solid #32302f;
  }

  .signature-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0;
    font-size: 16.8px;
    margin-bottom: 9px;
    color: #e0e0e0;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .sig-name {
    font-weight: 600;
    color: #b8bb26;
    margin-right: 2px;
  }

  .arg {
    display: flex;
    align-items: center;
  }

  .arg-name {
    color: #83a598;
  }

  .arg-types {
    color: #fabd2f;
  }

  .arg-default {
    color: #d3869b;
  }

  .return-type {
    color: #fabd2f;
  }

  .arg-name,
  .arg-types,
  .return-type {
    overflow-wrap: break-word;
    white-space: nowrap;
  }

  .sig-description {
    margin-bottom: 8px;
    color: #b8b8d0;
    font-size: 15.4px;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .args-list {
    padding-top: 8px;
    padding-bottom: 2px;

    ul {
      margin-top: 8px;
      margin-bottom: 2px;
    }
  }

  .args-title {
    font-weight: 600;
    margin-bottom: 0;
    color: #e0e0e0;
  }

  .arg-desc {
    color: #bbbbbb;
    font-size: 15.4px;
  }

  .arg-default::before {
    content: '=';
    color: #fe8019;
    margin: 0 4px 4px;
    font-style: normal;
  }

  @media (max-width: 600px) {
    .root {
      padding: 4px 12px 8px 12px;
    }

    .fn-signature {
      margin-left: 8px;
      margin-bottom: 8px;
      padding: 6px 8px;
    }

    .fn-name {
      font-size: 24px;
      scroll-margin-top: 110px;
    }

    .signature-line {
      font-size: 14px;
    }

    .arg-description {
      font-size: 14px;
    }

    .sig-description {
      font-size: 14px;
    }
  }
</style>
