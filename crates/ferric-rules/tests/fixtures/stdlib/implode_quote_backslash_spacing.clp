
(defrule probe =>
(printout t "[" (implode$ (create$ "a\"b" "a\\b" "\"" "\\" "a\\\"b" "\\\\" " leading " "trailing ")) "]" crlf)
)
