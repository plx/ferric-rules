(deffunction show (?label ?fields)
 (printout t ?label ":" (length$ ?fields) crlf)
 (progn$ (?field ?fields)
  (printout t (integerp ?field) ":" (floatp ?field) ":" (stringp ?field) ":" (symbolp ?field) ":[" ?field "]" crlf)))
(defrule probe =>
 (show delimiters (explode$ "(a b) ?x $?x ?*x* ? $? = <- & | ~ :"))
 (show within (explode$ "MAIN::name a:b a&b a|b a~b a\\b red;comment
end"))
)
