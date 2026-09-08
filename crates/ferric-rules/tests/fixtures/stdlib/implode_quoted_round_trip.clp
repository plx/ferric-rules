
(defrule probe =>
(printout t (eq (explode$ (implode$ (create$ a "two words" "a\"b" "a\\b" 3))) (create$ a "two words" "a\"b" "a\\b" 3)) crlf)
)
