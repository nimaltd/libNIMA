<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import kofiQr from "./kofi-qr.svg";
  import paypalQr from "./paypal-donate-qr.svg";

  // The welcome page's way to give: PayPal and Ko-fi, each by its link from here
  // or by its QR code from a phone. All four NIMA apps show the same.
  let {
    app,
    size = 240,
  }: {
    app: string;
    /** How wide each QR card is drawn, in CSS pixels: 240 on screen in all four apps, as big as fits beside terNIMA's
     * welcome text in its first window, 1200 pixels wide. An app whose window is zoomed gives less, to make up for it. */
    size?: number;
  } = $props();

  const GIFTS = [
    { name: "PayPal", url: "https://www.paypal.com/donate/?business=nima.askari@gmail.com", qr: paypalQr },
    { name: "Ko-fi", url: "https://ko-fi.com/nimaltd", qr: kofiQr },
  ];
</script>

<aside class="donate" style:--qr={`${size}px`}>
  <p class="title">Donate</p>
  <p>{app} is free.<br />If it saves you time:</p>
  <div class="gifts">
    {#each GIFTS as gift (gift.name)}
      <div class="gift">
        <button class="qr" title="Give through {gift.name}" onclick={() => void openUrl(gift.url)}>
          <!-- The cards are 990 by 1193. -->
          <img src={gift.qr} width={size} height={Math.round((size * 1193) / 990)} alt="{gift.name} QR code for Nima Askari" />
        </button>
        <button class="link" onclick={() => void openUrl(gift.url)}>{gift.name}</button>
      </div>
    {/each}
  </div>
</aside>

<style>
  /* Two cards and the gap between them wide: the line above them wraps in a narrow one. */
  .donate {
    max-width: calc(2 * var(--qr) + 32px);
    font-family: var(--font-mono);
    font-size: 14px;
    font-variant-ligatures: none;
    line-height: 18px;
    color: var(--muted);
    text-align: center;
  }

  p {
    margin: 0;
  }

  .title {
    margin-bottom: 4px;
    font-family: var(--font-ui);
    font-size: 18px;
    font-weight: 600;
    line-height: 24px;
    color: var(--accent);
  }

  .gifts {
    display: flex;
    justify-content: center;
    gap: 32px;
    margin-top: 10px;
  }

  .gift {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }

  .qr {
    display: block;
    padding: 0;
    background: none;
    border: 0;
    cursor: pointer;
  }

  .qr img {
    display: block;
    width: var(--qr);
    height: auto;
    border-radius: calc(var(--qr) * 0.07);
  }

  .link {
    padding: 0;
    font: inherit;
    color: var(--muted);
    background: none;
    border: 0;
    cursor: pointer;
  }

  .link:hover {
    color: var(--accent);
    text-decoration: underline;
  }
</style>
