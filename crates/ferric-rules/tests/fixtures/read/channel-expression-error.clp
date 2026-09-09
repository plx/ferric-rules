(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defglobal ?*trace* = 0 ?*result* = old)
(deffunction channel () (bind ?*trace* (+ ?*trace* 1)) (/ 1 0))
(defrule probe => (bind ?*result* (read (channel))) (printout t "after" crlf))
