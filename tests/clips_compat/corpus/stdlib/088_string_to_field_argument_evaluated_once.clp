(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
  (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defglobal ?*trace* = 0)
(deffunction mark (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
(defrule probe =>
 (show parsed (string-to-field (mark "42 rest")))
 (printout t "trace:" ?*trace* crlf)
 (bind ?*trace* 0)
 (show empty (string-to-field (mark "")))
 (printout t "trace:" ?*trace* crlf))
