
(defrule probe =>
(printout t "[" (implode$ (create$ (create$ a "b c") (create$) (create$ d "e"))) "]" crlf)
)
