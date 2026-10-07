(deffacts seed (prior 1) (prior 2))
(deffunction clear-now () (printout t "clear:[" (clear) "] inner-after" crlf) done)
(defrule high (declare (salience 10)) =>
  (printout t "outer:[" (clear-now) "] outer-after" crlf)
  (printout t "fresh:" (assert (fresh)) crlf))
(defrule low => (printout t "low:" (assert (lower)) crlf))
