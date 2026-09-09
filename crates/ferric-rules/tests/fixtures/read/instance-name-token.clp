(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe => (bind ?v (read)) (show value ?v) (printout t "instance-name:" (instance-namep ?v) crlf))
