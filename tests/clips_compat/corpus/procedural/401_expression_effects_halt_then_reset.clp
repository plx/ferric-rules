(deffunction stop () (halt) TRUE)
(defrule r =>
  (printout t "fire" crlf)
  (stop)
  (reset)
  (printout t "after" crlf))
(defrule s (declare (salience -10)) =>
  (printout t "unexpected-s" crlf))
