(defrule fail (declare (salience 10)) => (printout t prefix crlf) (log -1) (printout t AFTER crlf))
(defrule later => (printout t LATER crlf))
