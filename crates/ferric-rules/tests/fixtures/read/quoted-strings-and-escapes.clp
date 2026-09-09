(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show words (read))
 (show empty (read))
 (show quote (read))
 (show backslash (read))
 (show n (read))
 (show unknown (read))
 (show horizontal-tab (read))
 (show unicode (read))
)
