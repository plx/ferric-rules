; A nested run request has no effect while the engine is already running.
;; Level: interaction
;; Covers: agenda, run-from-rhs
; Protocol: load, reset, run to quiescence.
(deffacts input (ready))
(defrule first
  (declare (salience 10)) (ready)
  => (printout t "before" crlf) (run) (printout t "after" crlf))
(defrule second (ready) => (printout t "second" crlf))
