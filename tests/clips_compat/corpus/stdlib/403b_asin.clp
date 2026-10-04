(defrule fail (declare (salience 10)) => (printout t prefix crlf) (asin 2) (printout t AFTER crlf))
(defrule later => (printout t LATER crlf))
