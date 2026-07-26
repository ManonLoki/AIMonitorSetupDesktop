import type { HTMLAttributes, ReactNode } from "react";
import { useEffect, useRef } from "react";
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";

gsap.registerPlugin(ScrollTrigger);

interface AnimatedContentProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
  container?: Element | string | null;
  distance?: number;
  direction?: "vertical" | "horizontal";
  reverse?: boolean;
  duration?: number;
  ease?: string;
  initialOpacity?: number;
  animateOpacity?: boolean;
  scale?: number;
  threshold?: number;
  delay?: number;
  onComplete?: () => void;
}

/**
 * Adapted from ReactBits AnimatedContent (TypeScript + CSS variant).
 * https://reactbits.dev/animations/animated-content
 *
 * The local version keeps ReactBits' source-component integration model and
 * adds an explicit reduced-motion path for desktop accessibility.
 */
export function AnimatedContent({
  children,
  container,
  distance = 100,
  direction = "vertical",
  reverse = false,
  duration = 0.8,
  ease = "power3.out",
  initialOpacity = 0,
  animateOpacity = true,
  scale = 1,
  threshold = 0.1,
  delay = 0,
  onComplete,
  className = "",
  style,
  ...props
}: AnimatedContentProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;

    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      gsap.set(element, { visibility: "visible", clearProps: "transform,opacity" });
      onComplete?.();
      return;
    }

    let scrollerTarget: Element | string | null = container ?? null;
    if (typeof scrollerTarget === "string") {
      scrollerTarget = document.querySelector(scrollerTarget);
    }

    const axis = direction === "horizontal" ? "x" : "y";
    const offset = reverse ? -distance : distance;
    const startPercentage = (1 - threshold) * 100;

    gsap.set(element, {
      [axis]: offset,
      scale,
      opacity: animateOpacity ? initialOpacity : 1,
      visibility: "visible",
      willChange: "transform, opacity",
    });

    const timeline = gsap.timeline({
      paused: true,
      delay,
      onComplete: () => {
        gsap.set(element, { clearProps: "willChange" });
        onComplete?.();
      },
    });

    timeline.to(element, {
      [axis]: 0,
      scale: 1,
      opacity: 1,
      duration,
      ease,
    });

    const trigger = ScrollTrigger.create({
      trigger: element,
      scroller: scrollerTarget || window,
      start: `top ${startPercentage}%`,
      once: true,
      onEnter: () => timeline.play(),
    });

    return () => {
      trigger.kill();
      timeline.kill();
    };
  }, [
    animateOpacity,
    container,
    delay,
    direction,
    distance,
    duration,
    ease,
    initialOpacity,
    onComplete,
    reverse,
    scale,
    threshold,
  ]);

  return (
    <div
      ref={ref}
      className={className}
      style={{ visibility: "hidden", ...style }}
      {...props}
    >
      {children}
    </div>
  );
}
