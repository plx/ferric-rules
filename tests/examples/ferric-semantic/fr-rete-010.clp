; FR-RETE-010: reset creates the empty-LHS activation before deffacts, so
; depth fires the newer fact-backed activation first.
(deffacts startup
  (foo))

(defrule empty-prefix
  =>
  (printout t "E" crlf)
  (assert (result E)))

(defrule fact-backed
  (foo)
  =>
  (printout t "F" crlf)
  (assert (result F)))
