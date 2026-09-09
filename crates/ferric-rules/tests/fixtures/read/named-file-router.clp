(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (open "/work/named-file-router.in" source "r")
 (show one (read source)) (show two (read source))
 (show three (read source)) (show four (read source))
 (show five (read source)) (close source))
