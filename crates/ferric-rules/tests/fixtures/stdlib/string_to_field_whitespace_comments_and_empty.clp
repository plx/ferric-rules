(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (floatp ?value) ":"
  (stringp ?value) ":" (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show empty (string-to-field ""))
 (show space (string-to-field " 	
 "))
 (show comment (string-to-field "; just a comment"))
 (show after-comment (string-to-field "; comment
 42 tail"))
 (show many-comments (string-to-field " ;one
 ;two
 red tail"))
 (show nbsp (string-to-field " abc tail"))
)
