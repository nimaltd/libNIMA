// A question put before something that cannot be taken back. App.svelte shows it.

export type Confirm = {
  title: string;
  text: string;
  /** What the button that goes ahead says. */
  yes: string;
  then: () => void;
};

export const asking = $state({ now: null as Confirm | null });

/** Asks, and goes ahead on a yes. */
export function confirmFirst(title: string, text: string, yes: string, then: () => void): void {
  asking.now = { title, text, yes, then };
}
