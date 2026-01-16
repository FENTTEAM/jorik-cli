/**
 * confetti.js
 *
 * Provides a `startConfetti()` helper to run a short celebratory animation.
 * If `confetti` (e.g., canvas-confetti) is not present, this script will attempt
 * to load it from a CDN automatically.
 *
 * Usage (browser):
 *   <script src="/confetti.js"></script>
 *   <script>startConfetti();</script>
 *
 * The function is exposed as a global `startConfetti` and also as CommonJS export
 * when available (for use in bundlers/Node).
 */
(function (global) {
  "use strict";

  function ensureConfettiLoaded(cb) {
    if (typeof global.confetti === "function") {
      cb();
      return;
    }

    if (typeof document === "undefined") {
      console.warn("confetti is not available in this environment.");
      return;
    }

    // If script already injected, wait until it's available.
    const existing = document.querySelector('script[data-confetti]');
    if (existing) {
      const wait = setInterval(function () {
        if (typeof global.confetti === "function") {
          clearInterval(wait);
          cb();
        }
      }, 50);
      return;
    }

    const script = document.createElement("script");
    script.src = "https://cdn.jsdelivr.net/npm/canvas-confetti@1.6.0/dist/confetti.browser.min.js";
    script.async = true;
    script.setAttribute("data-confetti", "true");
    script.onload = cb;
    script.onerror = function () {
      console.warn("Failed to load canvas-confetti from CDN.");
    };
    (document.head || document.documentElement).appendChild(script);
  }

  /**
   * Start the confetti animation using the provided configuration.
   * The animation runs for ~15s and fires bursts every 250ms.
   */
  function startConfetti() {
    ensureConfettiLoaded(function () {
      const duration = 15 * 1000,
        animationEnd = Date.now() + duration,
        defaults = { startVelocity: 30, spread: 360, ticks: 60, zIndex: 0 };

      function randomInRange(min, max) {
        return Math.random() * (max - min) + min;
      }

      const interval = setInterval(function () {
        const timeLeft = animationEnd - Date.now();

        if (timeLeft <= 0) {
          return clearInterval(interval);
        }

        const particleCount = 50 * (timeLeft / duration);

        // left half
        global.confetti(
          Object.assign({}, defaults, {
            particleCount,
            origin: { x: randomInRange(0.1, 0.3), y: Math.random() - 0.2 },
          })
        );
        // right half
        global.confetti(
          Object.assign({}, defaults, {
            particleCount,
            origin: { x: randomInRange(0.7, 0.9), y: Math.random() - 0.2 },
          })
        );
      }, 250);

      // Expose a simple stop helper (overwrites previous if any)
      global.__stopConfetti = function () {
        clearInterval(interval);
      };
    });
  }

  // Expose globally and as CommonJS module if available
  global.startConfetti = startConfetti;
  if (typeof module !== "undefined" && module.exports) {
    module.exports = startConfetti;
  }
})(typeof window !== "undefined" ? window : typeof global !== "undefined" ? global : this);
