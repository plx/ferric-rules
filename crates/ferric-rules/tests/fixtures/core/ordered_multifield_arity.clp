; An anonymous multifield wildcard accepts zero, one or many fields.
;; Level: boundary
;; Covers: patterns, anonymous-multifield
; Protocol: load, reset, run to quiescence.
(deffacts input (row) (row a) (row a b) (ready))
(defglobal ?*matches* = 0)
(defrule count-rows
  (declare (salience 10)) (row $?)
  => (bind ?*matches* (+ ?*matches* 1)))
(defrule observe (ready) => (printout t ?*matches* crlf))
