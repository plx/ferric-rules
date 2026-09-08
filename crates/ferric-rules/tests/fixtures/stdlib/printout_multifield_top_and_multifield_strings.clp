
(defrule exercise =>
(printout t "top:[" "" "]|mf:" (create$ "") crlf)
(printout t "top:[" "a" "]|mf:" (create$ "a") crlf)
(printout t "top:[" "two words" "]|mf:" (create$ "two words") crlf)
(printout t "top:[" " leading " "]|mf:" (create$ " leading ") crlf)
(printout t "top:[" "a\"b" "]|mf:" (create$ "a\"b") crlf)
(printout t "top:[" "a\\b" "]|mf:" (create$ "a\\b") crlf)
(printout t "top:[" "\"" "]|mf:" (create$ "\"") crlf)
(printout t "top:[" "\\" "]|mf:" (create$ "\\") crlf)
)
