(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
 (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(deffunction next-value () (read))
(defgeneric next-method-value)
(defmethod next-method-value () (read))
(defrule probe =>
 (show function (next-value)) (show method (next-method-value))
 (show nested (str-cat "[" (read) "]")))
