(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defglobal ?*trace* = 0 ?*result* = old)
(deffunction channel (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
(defrule probe => (bind ?*result* (read (channel missing))) (printout t "after" crlf))
