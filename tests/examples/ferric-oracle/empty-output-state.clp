; Structured-oracle control fixture: semantic work is visible in final state,
; while the user-visible output channel is intentionally empty.

(deffacts setup
   (input 1))

(defrule compute-result
   ?input <- (input 1)
   =>
   (retract ?input)
   (assert (result 42)))
