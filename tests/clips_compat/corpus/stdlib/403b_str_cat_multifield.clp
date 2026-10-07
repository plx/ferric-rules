(defrule fail (declare (salience 10)) => (printout t prefix crlf) (str-cat (create$ a b) (printout t LATE)) (printout t AFTER crlf))
(defrule later => (printout t LATER crlf))
