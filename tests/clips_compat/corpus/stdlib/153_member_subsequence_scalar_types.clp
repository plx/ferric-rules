(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show integer (member$ 2 (create$ 2.0 2 "2")))
 (show float (member$ 2.0 (create$ 2 2.0 "2")))
 (show string (member$ "red" (create$ red "red")))
 (show symbol (member$ red (create$ "red" red)))
 (show false (member$ FALSE (create$ TRUE FALSE)))
 (show string-case (member$ "A" (create$ "a")))
 (show symbol-case (member$ A (create$ a)))
 (show signed-zero (member$ -0.0 (create$ 0.0 -0.0)))
 (show large-integer (member$ 9007199254740993 (create$ 9007199254740992 9007199254740993)))
)
