(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show suffix (read))
 (show dot-suffix (read))
 (show exp (read))
 (show exp-sign (read))
 (show two-dots (read))
 (show nan (read))
 (show inf (read))
 (show hex (read))
)
