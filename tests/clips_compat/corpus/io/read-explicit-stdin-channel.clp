(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defglobal ?*trace* = 0)
(deffunction channel (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
(defrule probe =>
 (show t (read (channel t))) (show stdin (read (channel stdin)))
 (show string (read (channel "stdin"))) (show string-t (read (channel "t")))
 (printout t "trace:" ?*trace* crlf))
