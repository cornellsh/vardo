import { useEffect, useRef } from 'react';

/**
 * Lock after a period with no user input.
 *
 * Uses a wall-clock deadline rather than a `setTimeout` that gets reset,
 * because a suspended laptop does not fire timers: on resume, a naive timeout
 * would happily continue counting from where it paused and leave the vault
 * unlocked for far longer than intended. Checking the clock on a tick means a
 * machine that slept through the window locks on the first tick after waking.
 */
export function useAutoLock(minutes: number, onLock: () => void) {
  const deadline = useRef(Date.now() + minutes * 60_000);
  const fired = useRef(false);

  useEffect(() => {
    const extend = () => {
      deadline.current = Date.now() + minutes * 60_000;
    };

    const events = ['pointerdown', 'keydown', 'wheel', 'pointermove'] as const;
    for (const e of events) window.addEventListener(e, extend, { passive: true });

    const tick = window.setInterval(() => {
      if (!fired.current && Date.now() >= deadline.current) {
        fired.current = true;
        onLock();
      }
    }, 1000);

    // Locking on hide would be hostile — alt-tabbing away is not walking away —
    // but coming back is a good moment to notice the window elapsed.
    const onVisible = () => {
      if (!fired.current && document.visibilityState === 'visible' && Date.now() >= deadline.current) {
        fired.current = true;
        onLock();
      }
    };
    document.addEventListener('visibilitychange', onVisible);

    return () => {
      for (const e of events) window.removeEventListener(e, extend);
      document.removeEventListener('visibilitychange', onVisible);
      window.clearInterval(tick);
    };
  }, [minutes, onLock]);
}
