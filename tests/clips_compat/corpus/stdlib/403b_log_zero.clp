(defrule fail (declare (salience 10)) => (printout t prefix crlf) (log 0) (printout t AFTER crlf))
(defrule later => (printout t LATER crlf))
