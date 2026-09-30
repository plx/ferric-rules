(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
  (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show words (string-to-field "\"two words\" trailing"))
 (show empty (string-to-field "\"\" trailing"))
 (show quote (string-to-field "\"a\\\"b\" rest"))
 (show backslash (string-to-field "\"a\\\\b\" rest"))
 (show backslash-n (string-to-field "\"a\\nb\" rest"))
 (show backslash-t (string-to-field "\"a\\tb\" rest"))
 (show unknown-escape (string-to-field "\"a\\qb\" rest"))
 (show newline (string-to-field "\"a
b\" rest"))
)
