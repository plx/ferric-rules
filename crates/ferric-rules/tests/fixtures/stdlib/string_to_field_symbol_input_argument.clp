(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
  (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defglobal ?*trace* = 0)
(deffunction mark (?value) (bind ?*trace* (+ ?*trace* 1)) ?value)
(defrule probe =>
 (show parsed (string-to-field (mark red)))
 (show generated (string-to-field (mark (sym-cat "42 trailing"))))
 (printout t "trace:" ?*trace* crlf))
