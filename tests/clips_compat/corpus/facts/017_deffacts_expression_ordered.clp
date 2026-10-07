;; Ordered seed fields evaluate expressions and splice multifield results on reset.
;; Level: interaction
;; Covers: assertion-expression, deffacts, defglobal, create$
(defglobal ?*g* = 5)
(deffunction values () (create$ a (create$ b)))
(deffacts seed (p (+ 1 2) ?*g* q) (r (values) (create$) c))
(defrule p (declare (salience 10)) (p $?fields) => (printout t "p " ?fields crlf))
(defrule r (r $?fields) => (printout t "r " ?fields crlf))
