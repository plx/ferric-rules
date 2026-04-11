;; A rule that fires many times for cancellation testing.
;; Assert (counter 0) and reset to start the loop.
(defrule increment-counter
  ?f <- (counter ?n)
  =>
  (retract ?f)
  (assert (counter (+ ?n 1))))
