(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (open "/work/named-file-multiline-quote.in" source "r")
 (show one (read source)) (show two (read source)) (close source))
