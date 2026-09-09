(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show numeric (member$ (create$ 2 3.0) (create$ 2.0 3.0 2 3.0)))
 (show string-symbol (member$ (create$ "red" blue) (create$ red blue "red" blue)))
 (show mixed-missing (member$ (create$ 2.0 "red") (create$ 2 "red")))
 (show false-symbol (member$ (create$ FALSE) (create$ TRUE FALSE)))
 (show signed-zero (member$ (create$ -0.0 x) (create$ 0.0 x -0.0 x)))
 (show large-integer (member$ (create$ 9007199254740993 x) (create$ 9007199254740992 x 9007199254740993 x)))
)
