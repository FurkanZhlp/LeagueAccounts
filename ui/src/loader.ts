import lottie from "lottie-web/build/player/lottie_light";
// rcp-fe-lol-shared-components hextech-loading-animation ("loader_large_v2", 300×300, 30 fps),
// the animation the client shows while a screen loads. Vector only; no image assets.
import animationData from "./assets/hextech-loader.json";

/** Play the client's hextech loading animation in `container`; returns a disposer. */
export function hextechLoader(container: HTMLElement): () => void {
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const animation = lottie.loadAnimation({ container, renderer: "svg", loop: true, autoplay: !still, animationData });
  // Reduced motion: hold a frame where the diamonds are assembled.
  if (still) animation.goToAndStop(20, true);
  return () => animation.destroy();
}
