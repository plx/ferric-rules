; Higher salience runs first, including positive, zero and negative values.
;; Level: basic
;; Covers: agenda, salience-precedence
; Protocol: load, reset, run to quiescence.
(deffacts input (ready))
(defrule low (declare (salience -10)) (ready) => (printout t "low" crlf))
(defrule middle (ready) => (printout t "middle" crlf))
(defrule high (declare (salience 10)) (ready) => (printout t "high" crlf))
