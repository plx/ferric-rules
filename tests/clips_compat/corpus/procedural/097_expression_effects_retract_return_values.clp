(defrule run =>
  (bind ?f (assert (p)))
  (printout t "live:[" (retract ?f) "] missing:[" (retract 9) "] stale:[" (retract ?f) "]" crlf)
  (printout t "continued" crlf))
