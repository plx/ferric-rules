; A disjunction accepts either literal and excludes other values.
;; Level: boundary
;; Covers: patterns, disjoined-literals
; Protocol: load, reset, run to quiescence.
(deffacts input (color red) (color blue) (color green) (done))
(defglobal ?*matches* = 0)
(defrule count-colors
  (declare (salience 10)) (color red|blue)
  => (bind ?*matches* (+ ?*matches* 1)))
(defrule observe (done) => (printout t ?*matches* crlf))
