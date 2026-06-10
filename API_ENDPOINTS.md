# API Endpoints

## Login Endpoint

### POST `/api/login`

Accepts JSON Body

```json
{
    "email": "example@example.com",
    "password": "1234"
}
```

Responds with JSON

```json
{
    "msg": "OK",
    "moodleSessionKey": "abcdef"
}
```

## Data Endpoints

You must set the `Authorization` Header to the value of `moodleSessionKey` returned from login endpoint for all of those data endpoints 

|method|endpoint|brief|
|---|---|---|
|GET|`/api/me`|returns info about the logged in student (like Student ID, semester number, etc)
|GET|`/api/course_list`|returns list of available courses (won't return hidden courses)
|GET|`/api/course?id=<course_id>`|returns course content as a list of page fragments|
|GET|`/api/course_grades?id=<course_id>`|returns course grades as a tree like structure correspoinding to grades view on moodle
|GET|`/api/resource?id=<resource_id>&kind=<resource_kind>`|returns resource - not JSON, watch Content-Type header. Might also return redirect to external URL
|GET|`/api/proxy?path=<path>`|appends <path> query param to `https://ekursy.put.poznan.pl/` and returns raw output
|GET|`/api/forum?id=<forum_id>`|returns forum details and lists discussion threads (titles and IDs)
|GET|`/api/forum/discussion?id=<discussion_id>`|returns a list of discussion thread posts (subject, author, timestamp, content)

