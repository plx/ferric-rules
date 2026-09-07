; RH-CORE-015: static salience takes precedence over assertion recency.
(deffacts seed (old) (new))
(defrule older-high (declare (salience 100)) (old) => (printout t "high" crlf) (assert (result high)))
(defrule newer-low (declare (salience -10)) (new) => (printout t "low" crlf) (assert (result low)))
