
(defrule probe =>
(printout t "[" (implode$ (create$)) "]" crlf)
(printout t "[" (implode$ (create$ "")) "]" crlf)
(printout t "[" (implode$ (create$ "a")) "]" crlf)
(printout t "[" (implode$ (create$ a)) "]" crlf)
(printout t "[" (implode$ (create$ "two words")) "]" crlf)
(printout t (stringp (implode$ (create$))) ":" (stringp (implode$ (create$ "a"))) crlf)
)
