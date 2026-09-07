; Halt stops later rule firings while the current RHS finishes.
;; Level: interaction
;; Covers: agenda, halt-finishes-rhs
; Protocol: load, reset, run to quiescence.
(deffacts input (ready))
(defrule stop
  (declare (salience 10)) (ready)
  => (printout t "before" crlf) (halt) (printout t "after" crlf))
(defrule later (ready) => (printout t "later" crlf))
