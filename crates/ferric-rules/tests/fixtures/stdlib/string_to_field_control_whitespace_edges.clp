(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
  (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show comment-cr (string-to-field "; comment42 tail"))
 (show number-vtab (string-to-field "427 tail"))
 (show symbol-vtab (string-to-field "redblue tail"))
 (show quoted-vtab (string-to-field "\"ab\" tail"))
)
