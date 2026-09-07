; Shared-variable joins distinguish integer and float values.
;; Level: boundary
;; Covers: patterns, join-type-sensitive
; Protocol: load, reset, run to quiescence.
(deffacts input (left 7) (right 7.0) (done))
(defglobal ?*matches* = 0)
(defrule join-values
  (declare (salience 10)) (left ?value) (right ?value)
  => (bind ?*matches* (+ ?*matches* 1)))
(defrule observe (done) => (printout t ?*matches* crlf))
