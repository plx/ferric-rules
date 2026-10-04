(deffunction stop () (printout t "halt:[" (halt) "] inner-after" crlf) done)
(defrule high (declare (salience 10)) => (printout t "outer:[" (stop) "] outer-after" crlf))
(defrule low => (printout t "unexpected-low" crlf))
