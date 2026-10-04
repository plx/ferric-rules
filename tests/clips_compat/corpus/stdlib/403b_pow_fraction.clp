(defrule fail (declare (salience 10)) => (printout t prefix crlf) (** -8 0.5) (printout t AFTER crlf))
(defrule later => (printout t LATER crlf))
